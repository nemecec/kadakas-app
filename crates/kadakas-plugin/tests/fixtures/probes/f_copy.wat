(module (memory (export "memory") 1024)
  (func (export "run") (result i32) (local $i i32)
    (loop $l
      (memory.copy (i32.const 0) (i32.const 1) (i32.const 67108863))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $l (i32.lt_u (local.get $i) (i32.const 1000000))))
    (local.get $i)))
