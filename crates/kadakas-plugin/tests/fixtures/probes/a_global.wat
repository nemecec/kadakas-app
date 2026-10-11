(module (memory (export "memory") 1)
  (func (export "run") (result i32) (local i32)
    (loop $l
      (global.set 0 (i64.const 1000))
      (local.set 0 (i32.add (local.get 0) (i32.const 1)))
      (br_if $l (i32.lt_u (local.get 0) (i32.const 300000000))))
    (local.get 0)))
