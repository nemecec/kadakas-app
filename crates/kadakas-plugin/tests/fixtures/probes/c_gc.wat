(module (memory (export "memory") 1)
 (type $a (array (mut i8)))
 (func (export "run") (result i32) (local $arr (ref null $a)) (local $i i32)
   (local.set $arr (array.new_default $a (i32.const 100000000)))
   (loop $l
     (array.fill $a (local.get $arr) (i32.const 0) (i32.const 1) (i32.const 100000000))
     (local.set $i (i32.add (local.get $i) (i32.const 1)))
     (br_if $l (i32.lt_u (local.get $i) (i32.const 20))))
   (local.get $i)))
