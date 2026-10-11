;; A valid plug-in module whose `reset` never ends. The host must stop it with its fuel (or its
;; deadline) and answer with a limit error. Used by the browser test of the kadakas-server
;; repository, and by tests of the host.
(module
  (memory (export "memory") 2)
  (data (i32.const 1024) "{\"id\":\"test.endless\",\"name\":\"Endless loop\",\"version\":\"1\",\"abi_version\":2,\"kinds\":[\"object\"],\"object\":{\"shape_type\":\"point\"}}")
  (func (export "alloc") (param i32) (result i32) (i32.const 65536))
  (func (export "dealloc") (param i32 i32))
  (func (export "describe") (result i64)
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 124)))
  (func (export "reset") (param i32 i32) (result i64)
    (loop $l (br $l))
    (i64.const 0))
  (func (export "tool") (param i32 i32) (result i64) (i64.const 0)))
