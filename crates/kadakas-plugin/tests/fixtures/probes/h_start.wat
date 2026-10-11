(module (memory (export "memory") 1)
  (func $s (local $i i32) (loop $l (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $l)))
  (start $s)
  (func (export "run") (result i32) (i32.const 1)))
