(module (memory (export "memory") 1) (table $t 0 funcref)
  (func (export "run") (result i32) (local $i i32)
    (loop $l
      (drop (table.grow $t (ref.null func) (i32.const 100000)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $l (i32.lt_u (local.get $i) (i32.const 1000000))))
    (table.size $t)))
