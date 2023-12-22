(module
  (type (;0;) (func (param i32 i32 i32)))
  (type (;1;) (func))
  (import "env" "_crypto_keccak256" (func (;0;) (type 0)))
  (func (;1;) (type 1)
    (local i32 i64 i64 i64 i32 i32 i32 i64)
    global.get 0
    i32.const 32
    i32.sub
    local.tee 0
    global.set 0
    i32.const 32792
    i32.const 0
    i64.load offset=32768
    local.tee 1
    i32.wrap_i64
    i32.sub
    i64.load align=1
    local.set 2
    i32.const 0
    local.get 1
    i64.const 32
    i64.shl
    local.tee 3
    i64.const -137438953472
    i64.add
    i64.const 32
    i64.shr_s
    local.tee 1
    i64.store offset=32768
    i32.const 32792
    local.get 1
    i32.wrap_i64
    i32.sub
    i64.load align=1
    local.set 1
    i32.const 0
    local.get 3
    i64.const -274877906944
    i64.add
    i64.const 32
    i64.shr_s
    i64.store offset=32768
    local.get 0
    i32.const 24
    i32.add
    local.tee 4
    i64.const 0
    i64.store
    local.get 0
    i32.const 16
    i32.add
    local.tee 5
    i64.const 0
    i64.store
    local.get 0
    i32.const 8
    i32.add
    local.tee 6
    i64.const 0
    i64.store
    local.get 0
    i64.const 0
    i64.store
    local.get 1
    i64.const 8
    i64.shr_u
    i64.const 4278190080
    i64.and
    local.get 1
    i64.const 24
    i64.shr_u
    i64.const 16711680
    i64.and
    i64.or
    local.get 1
    i64.const 40
    i64.shr_u
    i64.const 65280
    i64.and
    local.get 1
    i64.const 56
    i64.shr_u
    i64.or
    i64.or
    i32.wrap_i64
    local.get 2
    i64.const 8
    i64.shr_u
    i64.const 4278190080
    i64.and
    local.get 2
    i64.const 24
    i64.shr_u
    i64.const 16711680
    i64.and
    i64.or
    local.get 2
    i64.const 40
    i64.shr_u
    i64.const 65280
    i64.and
    local.get 2
    i64.const 56
    i64.shr_u
    i64.or
    i64.or
    i32.wrap_i64
    local.get 0
    call 0
    i32.const 0
    i32.const 0
    i64.load offset=32768
    i64.const 32
    i64.shl
    i64.const 137438953472
    i64.add
    i64.const 32
    i64.shr_s
    local.tee 2
    i64.store offset=32768
    local.get 6
    i64.load
    local.set 1
    local.get 5
    i64.load
    local.set 3
    local.get 0
    i64.load
    local.set 7
    i32.const 32792
    local.get 2
    i32.wrap_i64
    local.tee 5
    i32.sub
    local.get 4
    i64.load
    i64.store align=1
    i32.const 32784
    local.get 5
    i32.sub
    local.get 3
    i64.store align=1
    i32.const 32776
    local.get 5
    i32.sub
    local.get 1
    i64.store align=1
    i32.const 32768
    local.get 5
    i32.sub
    local.get 7
    i64.store align=1
    local.get 0
    i32.const 32
    i32.add
    global.set 0)
  (memory (;0;) 16)
  (global (;0;) (mut i32) (i32.const 1048576))
  (global (;1;) i32 (i32.const 1048576))
  (global (;2;) i32 (i32.const 1048576))
  (export "memory" (memory 0))
  (export "system_keccak" (func 1))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2)))