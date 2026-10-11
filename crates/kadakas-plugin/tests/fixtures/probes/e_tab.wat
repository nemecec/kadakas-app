(module (memory (export "memory") 1) (table 10000000 funcref)
 (func (export "run") (result i32) (table.size 0)))