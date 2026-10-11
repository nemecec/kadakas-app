//! Limits for engines that have none of their own: the browser.
//!
//! `wasmtime` gives fuel, a memory cap and a deadline. The `WebAssembly` API of a browser gives
//! none of them. This module rewrites a module, before it is compiled, so that the rewritten
//! module enforces the limits itself:
//!
//! - **Fuel.** The rewrite adds a mutable `i64` global that starts at the fuel of the call. It
//!   charges the number of instructions of each straight run of code at its start (a run ends at
//!   a block, loop, `if`, `else`, `end`, branch, `return` or `unreachable`). At the start of every
//!   function and at the start of every loop body it also checks the global. When it is below
//!   zero, the module executes `unreachable`. Every endless loop passes a loop start, and every
//!   endless recursion passes a function start, so no module can run longer than its fuel.
//!   `memory.fill`, `memory.copy`, `memory.init` and the table versions are charged by size
//!   (one unit per 16 elements), as in wasmtime. Because every call runs in a new instance, the
//!   fuel of a call starts full.
//! - **Memory.** The memory section gets a maximum: the memory cap in pages. A module that needs
//!   more at the start is refused. A `memory.grow` over the cap returns -1, as in any engine.
//! - **No imports.** A module with an import is refused.
//!
//! - **Tables.** Every table gets a maximum of [`MAX_TABLE_ELEMENTS`] elements, a module whose table
//!   starts bigger is refused, and a module has at most [`MAX_TABLES`] tables (as in the native
//!   engine).
//! - **Validation first.** The input is *validated* with an explicit list of WebAssembly features
//!   ([`allowed_features`]) before it is rewritten, and the output is validated again. This is not
//!   optional: the fuel counter has the first free global index, so a module that is invalid
//!   (for example one that uses `global.set 0` with no globals of its own) could become valid
//!   after the rewrite and reset its own fuel. The list leaves out everything whose effects the
//!   rewrite does not charge or cap: exceptions (their handlers run without charges), garbage
//!   collected arrays (they bypass fuel and the memory cap), threads, memory64, custom page sizes,
//!   multiple memories, relaxed SIMD, wide arithmetic and stack switching.
//!
//! The global is exported as [`FUEL_EXPORT`], so the host can tell "out of fuel" from a trap of
//! the module: out of fuel means the global is below zero after the trap.
//!
//! The rewrite is pure. It runs and is tested on every target, including a native test engine
//! that executes the rewritten modules. The browser engine (`crate::web`) uses it.
//!
//! What this does not give: a wall-clock deadline. The count of instructions is exact, but the
//! time of one instruction depends on the computer. The deadline of 1.5 s that the native engine
//! enforces has no counterpart in a browser without a worker; see `docs/plugins.md`.

use wasm_encoder::reencode::{Error, Reencode};
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, ExportKind, ExportSection, GlobalSection, GlobalType,
    Instruction, MemoryType, Module, SectionId, ValType,
};
use wasmparser::{Operator, Parser, Payload, Validator, WasmFeatures};

use crate::runtime::PluginError;

/// The name under which the rewritten module exports its fuel global.
pub const FUEL_EXPORT: &str = "kadakas_fuel";

/// Size of a WebAssembly page in bytes.
pub const PAGE: u64 = 65_536;

/// Most elements that a table may hold. The same cap as the native engine.
pub const MAX_TABLE_ELEMENTS: u64 = 100_000;

/// Most tables in a module. The same as the native engine.
pub const MAX_TABLES: u32 = 4;

/// The WebAssembly features that a plug-in may use. This is the single list for both engines
/// (`Plugin::load` validates with it before any engine sees the module). It is an allowlist: a
/// feature that is not named here is off.
pub fn allowed_features() -> WasmFeatures {
    WasmFeatures::WASM1
        | WasmFeatures::BULK_MEMORY
        | WasmFeatures::REFERENCE_TYPES
        | WasmFeatures::SIGN_EXTENSION
        | WasmFeatures::SATURATING_FLOAT_TO_INT
        | WasmFeatures::MULTI_VALUE
        | WasmFeatures::SIMD
        | WasmFeatures::TAIL_CALL
}

/// Validates a module against [`allowed_features`]. Parsing alone does not check that a module is
/// well formed or that it stays inside the features: this does.
pub fn validate(wasm: &[u8]) -> Result<(), PluginError> {
    Validator::new_with_features(allowed_features())
        .validate_all(wasm)
        .map(|_| ())
        .map_err(|e| PluginError::Invalid(format!("the module is not valid: {e}")))
}

/// One unit of fuel pays for this many elements of a bulk operation.
const BULK_UNIT_SHIFT: i64 = 4;

/// What the rewrite needs to know.
#[derive(Clone, Copy, Debug)]
pub struct HardenLimits {
    /// Fuel of one call, in instructions.
    pub fuel: u64,
    /// Most bytes of linear memory.
    pub memory_bytes: usize,
}

#[derive(Debug)]
enum Fail {
    Invalid(String),
    Limit(String),
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fail::Invalid(m) | Fail::Limit(m) => f.write_str(m),
        }
    }
}

impl From<Fail> for PluginError {
    fn from(f: Fail) -> PluginError {
        match f {
            Fail::Invalid(m) => PluginError::Invalid(m),
            Fail::Limit(m) => PluginError::Limit(m),
        }
    }
}

struct Hardener {
    /// Index of the fuel global.
    fuel_global: u32,
    /// Index of the scratch global for the size of a bulk operation.
    tmp_global: u32,
    fuel: i64,
    max_pages: u64,
    has_global_section: bool,
    inserted_globals: bool,
}

/// What kind of instruction this is for the accounting.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Kind {
    /// The next instruction starts a new run.
    ends_run: bool,
    /// This instruction starts a loop body.
    loop_start: bool,
    /// This instruction has the number of elements on top of the stack.
    bulk: bool,
}

fn classify(op: &Operator<'_>) -> Kind {
    Kind {
        ends_run: matches!(
            op,
            Operator::Block { .. }
                | Operator::Loop { .. }
                | Operator::If { .. }
                | Operator::Else
                | Operator::End
                | Operator::Br { .. }
                | Operator::BrIf { .. }
                | Operator::BrTable { .. }
                | Operator::Return
                | Operator::Unreachable
        ),
        loop_start: matches!(op, Operator::Loop { .. }),
        bulk: matches!(
            op,
            Operator::MemoryFill { .. }
                | Operator::MemoryCopy { .. }
                | Operator::MemoryInit { .. }
                | Operator::TableFill { .. }
                | Operator::TableCopy { .. }
                | Operator::TableInit { .. }
        ),
    }
}

fn user<T>(f: Fail) -> Result<T, Error<Fail>> {
    Err(Error::UserError(f))
}

impl Hardener {
    fn add_globals(&self, globals: &mut GlobalSection) {
        globals.global(
            GlobalType {
                val_type: ValType::I64,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i64_const(self.fuel),
        );
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(0),
        );
    }

    fn charge(&self, f: &mut wasm_encoder::Function, cost: u32) {
        f.instruction(&Instruction::GlobalGet(self.fuel_global));
        f.instruction(&Instruction::I64Const(i64::from(cost)));
        f.instruction(&Instruction::I64Sub);
        f.instruction(&Instruction::GlobalSet(self.fuel_global));
    }

    fn check(&self, f: &mut wasm_encoder::Function) {
        f.instruction(&Instruction::GlobalGet(self.fuel_global));
        f.instruction(&Instruction::I64Const(0));
        f.instruction(&Instruction::I64LtS);
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::Unreachable);
        f.instruction(&Instruction::End);
    }

    /// Charges a bulk operation by its size. The size is on top of the stack and stays there.
    fn charge_bulk(&self, f: &mut wasm_encoder::Function) {
        f.instruction(&Instruction::GlobalSet(self.tmp_global));
        f.instruction(&Instruction::GlobalGet(self.fuel_global));
        f.instruction(&Instruction::GlobalGet(self.tmp_global));
        f.instruction(&Instruction::I64ExtendI32U);
        f.instruction(&Instruction::I64Const(BULK_UNIT_SHIFT));
        f.instruction(&Instruction::I64ShrU);
        f.instruction(&Instruction::I64Sub);
        f.instruction(&Instruction::GlobalSet(self.fuel_global));
        f.instruction(&Instruction::GlobalGet(self.tmp_global));
    }
}

impl Reencode for Hardener {
    type Error = Fail;

    fn memory_type(&mut self, t: wasmparser::MemoryType) -> Result<MemoryType, Error<Self::Error>> {
        if t.memory64 || t.shared || t.page_size_log2.is_some() {
            return user(Fail::Invalid(
                "the memory must be a plain 32-bit memory (no shared, 64-bit or custom page size)"
                    .into(),
            ));
        }
        if t.initial > self.max_pages {
            return user(Fail::Limit(format!(
                "the module needs {} pages of memory at the start. The limit is {}",
                t.initial, self.max_pages
            )));
        }
        Ok(MemoryType {
            minimum: t.initial,
            maximum: Some(t.maximum.map_or(self.max_pages, |m| m.min(self.max_pages))),
            memory64: false,
            shared: false,
            page_size_log2: None,
        })
    }

    fn table_type(
        &mut self,
        t: wasmparser::TableType,
    ) -> Result<wasm_encoder::TableType, Error<Self::Error>> {
        if t.table64 || t.shared {
            return user(Fail::Invalid("a table must be a plain 32-bit table".into()));
        }
        if t.initial > MAX_TABLE_ELEMENTS {
            return user(Fail::Limit(format!(
                "a table starts with {} elements. The limit is {MAX_TABLE_ELEMENTS}",
                t.initial
            )));
        }
        let mut out = wasm_encoder::reencode::utils::table_type(self, t)?;
        out.maximum = Some(
            t.maximum
                .map_or(MAX_TABLE_ELEMENTS, |m| m.min(MAX_TABLE_ELEMENTS)),
        );
        Ok(out)
    }

    fn parse_global_section(
        &mut self,
        globals: &mut GlobalSection,
        section: wasmparser::GlobalSectionReader<'_>,
    ) -> Result<(), Error<Self::Error>> {
        wasm_encoder::reencode::utils::parse_global_section(self, globals, section)?;
        self.add_globals(globals);
        self.inserted_globals = true;
        Ok(())
    }

    fn parse_export_section(
        &mut self,
        exports: &mut ExportSection,
        section: wasmparser::ExportSectionReader<'_>,
    ) -> Result<(), Error<Self::Error>> {
        wasm_encoder::reencode::utils::parse_export_section(self, exports, section)?;
        exports.export(FUEL_EXPORT, ExportKind::Global, self.fuel_global);
        Ok(())
    }

    /// A module without a global section gets one, in its place between the memory and the
    /// export sections.
    fn intersperse_section_hook(
        &mut self,
        module: &mut Module,
        _after: Option<SectionId>,
        before: Option<SectionId>,
    ) -> Result<(), Error<Self::Error>> {
        let after_globals = matches!(
            before,
            None | Some(
                SectionId::Export
                    | SectionId::Start
                    | SectionId::Element
                    | SectionId::DataCount
                    | SectionId::Code
                    | SectionId::Data
            )
        );
        if !self.has_global_section && !self.inserted_globals && after_globals {
            let mut globals = GlobalSection::new();
            self.add_globals(&mut globals);
            module.section(&globals);
            self.inserted_globals = true;
        }
        Ok(())
    }

    fn parse_function_body(
        &mut self,
        code: &mut CodeSection,
        func: wasmparser::FunctionBody<'_>,
    ) -> Result<(), Error<Self::Error>> {
        // First pass: the cost of each run of instructions.
        let mut kinds = Vec::new();
        let mut reader = func.get_operators_reader()?;
        while !reader.eof() {
            kinds.push(classify(&reader.read()?));
        }
        // (cost, check) per run. The first run is the start of the function, which is checked.
        let mut runs: Vec<(u32, bool)> = Vec::new();
        let (mut cost, mut check) = (0u32, true);
        for k in &kinds {
            cost = cost.saturating_add(1);
            if k.ends_run {
                runs.push((cost, check));
                cost = 0;
                check = k.loop_start;
            }
        }
        if cost > 0 {
            runs.push((cost, check));
        }
        // Second pass: the same instructions with the charges in front of each run.
        let mut f = self.new_function_with_parsed_locals(&func)?;
        let mut reader = func.get_operators_reader()?;
        let (mut run, mut at_start) = (0usize, true);
        for k in &kinds {
            if at_start {
                let (cost, check) = runs[run];
                self.charge(&mut f, cost);
                if check {
                    self.check(&mut f);
                }
                at_start = false;
            }
            if k.bulk {
                self.charge_bulk(&mut f);
            }
            f.instruction(&self.parse_instruction(&mut reader)?);
            if k.ends_run {
                run += 1;
                at_start = true;
            }
        }
        code.function(&f);
        Ok(())
    }
}

/// Rewrites a module so that it enforces the fuel and the memory cap itself. See the module
/// documentation. The result has the same exports plus [`FUEL_EXPORT`].
pub fn harden(wasm: &[u8], limits: &HardenLimits) -> Result<Vec<u8>, PluginError> {
    validate(wasm)?;
    let mut globals = 0u32;
    let mut has_global_section = false;
    for payload in Parser::new(0).parse_all(wasm) {
        match payload.map_err(|e| PluginError::Invalid(e.to_string()))? {
            Payload::ImportSection(r) if r.count() > 0 => {
                return Err(PluginError::Invalid(
                    "the module has imports. A plug-in may import nothing".into(),
                ));
            }
            Payload::TableSection(r) if r.count() > MAX_TABLES => {
                return Err(PluginError::Limit(format!(
                    "the module has {} tables. The limit is {MAX_TABLES}",
                    r.count()
                )));
            }
            Payload::GlobalSection(r) => {
                has_global_section = true;
                globals = r.count();
            }
            Payload::ExportSection(r) => {
                for e in r {
                    if e.map_err(|e| PluginError::Invalid(e.to_string()))?.name == FUEL_EXPORT {
                        return Err(PluginError::Invalid(format!(
                            "the module exports '{FUEL_EXPORT}'. The host uses that name"
                        )));
                    }
                }
            }
            _ => {}
        }
    }
    let mut hardener = Hardener {
        fuel_global: globals,
        tmp_global: globals + 1,
        fuel: i64::try_from(limits.fuel).unwrap_or(i64::MAX),
        max_pages: limits.memory_bytes as u64 / PAGE,
        has_global_section,
        inserted_globals: false,
    };
    let mut module = Module::new();
    hardener
        .parse_core_module(&mut module, Parser::new(0), wasm)
        .map_err(|e| match e {
            Error::UserError(f) => PluginError::from(f),
            Error::ParseError(e) => PluginError::Invalid(e.to_string()),
            other => PluginError::Invalid(other.to_string()),
        })?;
    let out = module.finish();
    // The rewrite must give a valid module with the same features. If it does not, refuse.
    validate(&out)?;
    Ok(out)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use wasmtime::{Config, Engine, Instance, Linker, Module as WtModule, Store};

    const LIMITS: HardenLimits = HardenLimits {
        fuel: 100_000,
        memory_bytes: 4 * PAGE as usize,
    };

    fn wat(src: &str) -> Vec<u8> {
        wat::parse_str(src).unwrap()
    }

    /// Runs the rewritten module in a plain engine (no fuel, no deadline of its own).
    fn instantiate(wasm: &[u8]) -> (Store<()>, Instance) {
        let engine = Engine::new(&Config::new()).unwrap();
        let module = WtModule::new(&engine, wasm).expect("the rewritten module is valid");
        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine)
            .instantiate(&mut store, &module)
            .unwrap();
        (store, instance)
    }

    fn fuel_left(store: &mut Store<()>, instance: &Instance) -> i64 {
        instance
            .get_global(&mut *store, FUEL_EXPORT)
            .unwrap()
            .get(&mut *store)
            .i64()
            .unwrap()
    }

    #[test]
    fn a_plain_function_runs_the_same_and_costs_its_instructions() {
        let src = r#"(module
          (memory (export "memory") 1)
          (func (export "add") (param i32 i32) (result i32)
            (i32.add (local.get 0) (local.get 1))))"#;
        let hard = harden(&wat(src), &LIMITS).unwrap();
        let (mut store, instance) = instantiate(&hard);
        let add = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "add")
            .unwrap();
        assert_eq!(add.call(&mut store, (2, 3)).unwrap(), 5);
        // local.get, local.get, i32.add, end: four instructions.
        assert_eq!(fuel_left(&mut store, &instance), 100_000 - 4);
    }

    #[test]
    fn an_endless_loop_traps_with_the_fuel_below_zero() {
        let src = r#"(module (memory (export "memory") 1)
          (func (export "spin") (loop $l (br $l))))"#;
        let (mut store, instance) = instantiate(&harden(&wat(src), &LIMITS).unwrap());
        let spin = instance
            .get_typed_func::<(), ()>(&mut store, "spin")
            .unwrap();
        assert!(spin.call(&mut store, ()).is_err());
        assert!(fuel_left(&mut store, &instance) < 0);
    }

    #[test]
    fn loops_with_a_counter_and_branches_are_charged_per_run() {
        // Sum 1..=n with a loop, and use br_if, br_table, if/else and block.
        let src = r#"(module (memory (export "memory") 1)
          (func (export "sum") (param $n i32) (result i32) (local $i i32) (local $s i32)
            (block $done
              (loop $again
                (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
                (local.set $i (i32.add (local.get $i) (i32.const 1)))
                (if (i32.and (local.get $i) (i32.const 1))
                  (then (local.set $s (i32.add (local.get $s) (local.get $i))))
                  (else (local.set $s (i32.add (local.get $s) (local.get $i)))))
                (br $again)))
            (block $a (block $b (br_table $a $b (i32.const 1))))
            (local.get $s)))"#;
        let (mut store, instance) = instantiate(&harden(&wat(src), &LIMITS).unwrap());
        let sum = instance
            .get_typed_func::<i32, i32>(&mut store, "sum")
            .unwrap();
        assert_eq!(sum.call(&mut store, 10).unwrap(), 55);
        let used_10 = 100_000 - fuel_left(&mut store, &instance);
        assert!(used_10 > 10 * 10, "{used_10}");
        // More iterations cost more fuel; the loop is metered per iteration.
        let (mut store, instance) = instantiate(&harden(&wat(src), &LIMITS).unwrap());
        let sum = instance
            .get_typed_func::<i32, i32>(&mut store, "sum")
            .unwrap();
        sum.call(&mut store, 100).unwrap();
        assert!(100_000 - fuel_left(&mut store, &instance) > used_10 * 5);
        // 10 000 iterations need more than the fuel.
        let (mut store, instance) = instantiate(&harden(&wat(src), &LIMITS).unwrap());
        let sum = instance
            .get_typed_func::<i32, i32>(&mut store, "sum")
            .unwrap();
        assert!(sum.call(&mut store, 10_000).is_err());
        assert!(fuel_left(&mut store, &instance) < 0);
    }

    #[test]
    fn endless_recursion_and_tail_calls_run_out_of_fuel() {
        // Without a loop. A module of this kind would run for ever without a check at function starts.
        let src = r#"(module (memory (export "memory") 1)
          (func $f (call $f))
          (func (export "recurse") (call $f))
          (func $t (return_call $t))
          (func (export "tail") (return_call $t)))"#;
        let hard = harden(&wat(src), &LIMITS).unwrap();
        for name in ["recurse", "tail"] {
            let (mut store, instance) = instantiate(&hard);
            let f = instance.get_typed_func::<(), ()>(&mut store, name).unwrap();
            assert!(f.call(&mut store, ()).is_err(), "{name}");
            // The recursion may also end in a stack overflow before the fuel is gone.
            let left = fuel_left(&mut store, &instance);
            assert!(left < 0 || name == "recurse", "{name}: {left}");
        }
    }

    #[test]
    fn bulk_operations_are_charged_by_size() {
        let src = r#"(module (memory (export "memory") 1)
          (table 64 funcref)
          (elem funcref (ref.func $f))
          (func $f)
          (func (export "fill") (param i32)
            (memory.fill (i32.const 0) (i32.const 7) (local.get 0)))
          (func (export "copy") (param i32)
            (memory.copy (i32.const 0) (i32.const 1024) (local.get 0)))
          (func (export "tfill") (param i32)
            (table.fill 0 (i32.const 0) (ref.func $f) (local.get 0))))"#;
        let hard = harden(&wat(src), &LIMITS).unwrap();
        let (mut store, instance) = instantiate(&hard);
        let small = fuel_left(&mut store, &instance);
        assert_eq!(small, 100_000);
        let fill = instance
            .get_typed_func::<i32, ()>(&mut store, "fill")
            .unwrap();
        fill.call(&mut store, 1600).unwrap();
        let after_fill = fuel_left(&mut store, &instance);
        // 1600 bytes cost 100 units, plus the instructions around it.
        assert!((100_000 - after_fill) >= 100 && (100_000 - after_fill) < 130);
        // The memory really holds what was written.
        let memory = instance.get_memory(&mut store, "memory").unwrap();
        assert_eq!(memory.data(&store)[0], 7);
        let copy = instance
            .get_typed_func::<i32, ()>(&mut store, "copy")
            .unwrap();
        copy.call(&mut store, 6400).unwrap();
        assert!(after_fill - fuel_left(&mut store, &instance) >= 400);
        let tfill = instance
            .get_typed_func::<i32, ()>(&mut store, "tfill")
            .unwrap();
        let before = fuel_left(&mut store, &instance);
        tfill.call(&mut store, 64).unwrap();
        assert!(before - fuel_left(&mut store, &instance) >= 4);
    }

    #[test]
    fn the_memory_gets_a_maximum_and_growth_over_it_fails() {
        let src = r#"(module (memory (export "memory") 1)
          (func (export "grow") (param i32) (result i32) (memory.grow (local.get 0))))"#;
        let (mut store, instance) = instantiate(&harden(&wat(src), &LIMITS).unwrap());
        let grow = instance
            .get_typed_func::<i32, i32>(&mut store, "grow")
            .unwrap();
        // The cap is 4 pages in total: 3 more pages fit, a fourth does not.
        assert_eq!(grow.call(&mut store, 3).unwrap(), 1);
        assert_eq!(grow.call(&mut store, 1).unwrap(), -1);
        // A maximum that the module sets itself stays if it is lower.
        let low = r#"(module (memory (export "memory") 1 2)
          (func (export "grow") (param i32) (result i32) (memory.grow (local.get 0))))"#;
        let (mut store, instance) = instantiate(&harden(&wat(low), &LIMITS).unwrap());
        let grow = instance
            .get_typed_func::<i32, i32>(&mut store, "grow")
            .unwrap();
        assert_eq!(grow.call(&mut store, 2).unwrap(), -1);
        // A higher maximum is cut to the cap.
        let high = r#"(module (memory (export "memory") 1 100)
          (func (export "grow") (param i32) (result i32) (memory.grow (local.get 0))))"#;
        let (mut store, instance) = instantiate(&harden(&wat(high), &LIMITS).unwrap());
        let grow = instance
            .get_typed_func::<i32, i32>(&mut store, "grow")
            .unwrap();
        assert_eq!(grow.call(&mut store, 4).unwrap(), -1);
    }

    #[test]
    fn modules_that_break_the_rules_are_refused() {
        let big = r#"(module (memory (export "memory") 5))"#;
        assert!(matches!(
            harden(&wat(big), &LIMITS),
            Err(PluginError::Limit(m)) if m.contains("pages")
        ));
        let imports = r#"(module (import "env" "f" (func)) (memory (export "memory") 1))"#;
        assert!(matches!(
            harden(&wat(imports), &LIMITS),
            Err(PluginError::Invalid(m)) if m.contains("imports")
        ));
        let clash = r#"(module (memory (export "memory") 1) (global (export "kadakas_fuel") i32 (i32.const 0)))"#;
        assert!(matches!(
            harden(&wat(clash), &LIMITS),
            Err(PluginError::Invalid(m)) if m.contains("kadakas_fuel")
        ));
        let shared = r#"(module (memory (export "memory") 1 2 shared))"#;
        assert!(matches!(
            harden(&wat(shared), &LIMITS),
            Err(PluginError::Invalid(_))
        ));
        assert!(matches!(
            harden(b"junk", &LIMITS),
            Err(PluginError::Invalid(_))
        ));
        assert!(matches!(
            harden(
                &wat(r#"(module (memory (export "memory") 1))"#)[..9],
                &LIMITS
            ),
            Err(PluginError::Invalid(_))
        ));
    }

    #[test]
    fn globals_of_the_module_keep_their_indices_and_a_start_function_is_metered() {
        let src = r#"(module (memory (export "memory") 1)
          (global $g (mut i32) (i32.const 41))
          (global (export "answer") i32 (i32.const 1))
          (func $init (global.set $g (i32.add (global.get $g) (i32.const 1))))
          (start $init)
          (func (export "get") (result i32) (global.get $g)))"#;
        let (mut store, instance) = instantiate(&harden(&wat(src), &LIMITS).unwrap());
        let get = instance
            .get_typed_func::<(), i32>(&mut store, "get")
            .unwrap();
        assert_eq!(get.call(&mut store, ()).unwrap(), 42);
        // The start function used fuel as well.
        assert!(fuel_left(&mut store, &instance) < 100_000);
        // A start function that never ends cannot hang the instantiation.
        let hang = r#"(module (memory (export "memory") 1)
          (func $init (loop $l (br $l))) (start $init))"#;
        let hard = harden(&wat(hang), &LIMITS).unwrap();
        let engine = Engine::new(&Config::new()).unwrap();
        let module = WtModule::new(&engine, &hard).unwrap();
        let mut store = Store::new(&engine, ());
        assert!(
            Linker::new(&engine)
                .instantiate(&mut store, &module)
                .is_err()
        );
    }

    #[test]
    fn a_module_that_exports_nothing_still_gets_the_fuel_global() {
        // Not a plug-in, but the rewrite must still produce a valid module.
        let src = r#"(module (memory 1) (func (loop $l (br $l))))"#;
        let hard = harden(&wat(src), &LIMITS).unwrap();
        let engine = Engine::new(&Config::new()).unwrap();
        assert!(WtModule::new(&engine, &hard).is_ok());
    }

    #[test]
    fn the_real_example_works_after_the_rewrite() {
        let planter = include_bytes!("../tests/fixtures/raised_planter.wasm");
        let limits = HardenLimits {
            fuel: 200_000_000,
            memory_bytes: 64 << 20,
        };
        let hard = harden(planter, &limits).unwrap();
        let (mut store, instance) = instantiate(&hard);
        let describe = instance
            .get_typed_func::<(), u64>(&mut store, "describe")
            .unwrap();
        let packed = describe.call(&mut store, ()).unwrap();
        let (ptr, len) = kadakas_plugin_abi::guest::unpack(packed);
        let memory = instance.get_memory(&mut store, "memory").unwrap();
        let bytes = memory.data(&store)[ptr as usize..(ptr + len) as usize].to_vec();
        let manifest = kadakas_plugin_abi::Manifest::from_bytes(&bytes).unwrap();
        assert_eq!(manifest.id, "org.kadakas.raised-planter");
        let used = 200_000_000 - fuel_left(&mut store, &instance);
        assert!(used > 100 && used < 5_000_000, "{used}");
    }

    // ---- adversarial modules (tests/fixtures/probes, with the sources as .wat)

    const PROBE_GLOBAL: &str = include_str!("../tests/fixtures/probes/a_global.wat");
    const PROBE_EH: &str = include_str!("../tests/fixtures/probes/b_eh.wat");
    const PROBE_GC: &str = include_str!("../tests/fixtures/probes/c_gc.wat");
    const PROBE_MULTI_MEMORY: &str = include_str!("../tests/fixtures/probes/d_mm.wat");
    const PROBE_TABLE: &str = include_str!("../tests/fixtures/probes/e_tab.wat");

    #[test]
    fn probe_1_an_invalid_module_cannot_reach_the_fuel_global() {
        // It sets global 0, and has no global of its own: invalid. The rewrite would have made
        // global 0 the fuel counter and the module valid.
        let e = harden(&wat(PROBE_GLOBAL), &LIMITS);
        assert!(
            matches!(&e, Err(PluginError::Invalid(m)) if m.contains("not valid")),
            "{e:?}"
        );
        // The host refuses it before any engine sees it.
        assert!(crate::plugin::scan(&wat(PROBE_GLOBAL), &crate::Limits::default()).is_err());
    }

    #[test]
    fn probe_2_exception_handlers_are_refused() {
        assert!(matches!(
            harden(&wat(PROBE_EH), &LIMITS),
            Err(PluginError::Invalid(_))
        ));
        assert!(crate::plugin::scan(&wat(PROBE_EH), &crate::Limits::default()).is_err());
    }

    #[test]
    fn probe_3_garbage_collected_arrays_are_refused() {
        assert!(matches!(
            harden(&wat(PROBE_GC), &LIMITS),
            Err(PluginError::Invalid(_))
        ));
        assert!(crate::plugin::scan(&wat(PROBE_GC), &crate::Limits::default()).is_err());
    }

    #[test]
    fn probe_4_more_than_one_memory_and_huge_tables_are_refused() {
        assert!(matches!(
            harden(&wat(PROBE_MULTI_MEMORY), &LIMITS),
            Err(PluginError::Invalid(_))
        ));
        let e = harden(&wat(PROBE_TABLE), &LIMITS);
        assert!(
            matches!(&e, Err(PluginError::Limit(m)) if m.contains("table")),
            "{e:?}"
        );
        // Native engine and browser engine accept the same modules.
        assert!(crate::plugin::scan(&wat(PROBE_MULTI_MEMORY), &crate::Limits::default()).is_err());
    }

    #[test]
    fn tables_get_a_maximum_and_there_are_at_most_four() {
        let grow = r#"(module (memory (export "memory") 1) (table (export "t") 1 funcref)
          (func (export "grow") (param i32) (result i32) (table.grow 0 (ref.null func) (local.get 0))))"#;
        let (mut store, instance) = instantiate(&harden(&wat(grow), &LIMITS).unwrap());
        let f = instance
            .get_typed_func::<i32, i32>(&mut store, "grow")
            .unwrap();
        assert_eq!(f.call(&mut store, 100_000).unwrap(), -1);
        assert_eq!(f.call(&mut store, 10).unwrap(), 1);
        let five = r#"(module (memory (export "memory") 1)
          (table 1 funcref) (table 1 funcref) (table 1 funcref) (table 1 funcref) (table 1 funcref))"#;
        assert!(matches!(
            harden(&wat(five), &LIMITS),
            Err(PluginError::Limit(m)) if m.contains("tables")
        ));
        let four = r#"(module (memory (export "memory") 1)
          (table 1 funcref) (table 1 funcref) (table 1 funcref) (table 1 funcref))"#;
        assert!(harden(&wat(four), &LIMITS).is_ok());
        let big_max = r#"(module (memory (export "memory") 1) (table 1 5000000 funcref))"#;
        assert!(harden(&wat(big_max), &LIMITS).is_ok());
    }

    #[test]
    fn the_examples_pass_the_allowed_features() {
        for bytes in [
            &include_bytes!("../tests/fixtures/raised_planter.wasm")[..],
            &include_bytes!("../tests/fixtures/stone_row.wasm")[..],
        ] {
            validate(bytes).unwrap();
        }
        assert!(validate(b"\0asm\x01\0\0\0\x01").is_err());
    }
}
