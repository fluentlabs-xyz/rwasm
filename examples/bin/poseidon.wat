(module
  (type (;0;) (func (param i32 i32 i32) (result i32)))
  (type (;1;) (func (param i32 i32) (result i32)))
  (type (;2;) (func (param i32 i32 i32)))
  (type (;3;) (func (param i32 i32)))
  (type (;4;) (func (param i32)))
  (type (;5;) (func))
  (type (;6;) (func (param i32 i32 i32 i32) (result i32)))
  (type (;7;) (func (param i32 i32 i32 i32)))
  (import "env" "_sys_read" (func (;0;) (type 0)))
  (import "env" "_crypto_poseidon" (func (;1;) (type 2)))
  (import "env" "_sys_write" (func (;2;) (type 3)))
  (import "env" "_sys_halt" (func (;3;) (type 4)))
  (func (;4;) (type 5))
  (func (;5;) (type 5)
    (local i32 i32 i32 i32 i32 i32 i32)
    global.get 0
    i32.const 48
    i32.sub
    local.tee 0
    global.set 0
    local.get 0
    i32.const 40
    i32.add
    i32.const 0
    i32.store
    local.get 0
    i64.const 0
    i64.store offset=32
    local.get 0
    i32.const 32
    i32.add
    i32.const 96
    i32.const 12
    call 0
    drop
    local.get 0
    i32.const 0
    i32.store offset=8
    local.get 0
    i64.const 1
    i64.store align=4
    block  ;; label = @1
      local.get 0
      i32.load offset=32
      local.tee 1
      i32.eqz
      br_if 0 (;@1;)
      local.get 0
      local.get 1
      call 6
    end
    block  ;; label = @1
      block  ;; label = @2
        block  ;; label = @3
          block  ;; label = @4
            block  ;; label = @5
              block  ;; label = @6
                block  ;; label = @7
                  block  ;; label = @8
                    block  ;; label = @9
                      block  ;; label = @10
                        block  ;; label = @11
                          local.get 0
                          i32.load offset=40
                          local.tee 2
                          br_if 0 (;@11;)
                          local.get 0
                          i32.load offset=8
                          local.set 3
                          br 1 (;@10;)
                        end
                        local.get 0
                        i32.load offset=36
                        local.tee 3
                        local.get 2
                        i32.add
                        local.tee 4
                        i32.eqz
                        br_if 1 (;@9;)
                        local.get 4
                        i32.const -1
                        i32.le_s
                        br_if 2 (;@8;)
                        local.get 4
                        i32.const 65535
                        i32.add
                        i32.const 16
                        i32.shr_u
                        memory.grow
                        local.tee 1
                        i32.const -1
                        i32.eq
                        br_if 3 (;@7;)
                        local.get 1
                        i32.const 16
                        i32.shl
                        local.tee 1
                        i32.eqz
                        br_if 3 (;@7;)
                        local.get 1
                        i32.const 0
                        local.get 4
                        call 22
                        local.set 1
                        local.get 4
                        i32.const 11
                        i32.le_u
                        br_if 4 (;@6;)
                        local.get 1
                        local.get 0
                        i64.load offset=32
                        i64.store align=4
                        local.get 1
                        i32.const 8
                        i32.add
                        local.get 0
                        i32.const 32
                        i32.add
                        i32.const 8
                        i32.add
                        i32.load
                        i32.store
                        local.get 3
                        local.get 4
                        i32.gt_u
                        br_if 5 (;@5;)
                        local.get 1
                        local.get 3
                        i32.add
                        local.get 3
                        local.get 2
                        call 0
                        drop
                        block  ;; label = @11
                          local.get 1
                          i32.load
                          local.tee 3
                          br_if 0 (;@11;)
                          i32.const 0
                          local.set 3
                          br 1 (;@10;)
                        end
                        local.get 1
                        i32.load offset=8
                        local.tee 2
                        local.get 1
                        i32.load offset=4
                        local.tee 5
                        i32.add
                        local.tee 6
                        local.get 2
                        i32.lt_u
                        br_if 6 (;@4;)
                        local.get 6
                        local.get 4
                        i32.gt_u
                        br_if 7 (;@3;)
                        local.get 3
                        i32.const -1
                        i32.le_s
                        br_if 2 (;@8;)
                        i32.const 0
                        i32.load8_u offset=1049172
                        drop
                        local.get 3
                        i32.const 65535
                        i32.add
                        i32.const 16
                        i32.shr_u
                        memory.grow
                        local.tee 4
                        i32.const -1
                        i32.eq
                        br_if 8 (;@2;)
                        local.get 4
                        i32.const 16
                        i32.shl
                        local.tee 4
                        i32.eqz
                        br_if 8 (;@2;)
                        local.get 1
                        local.get 5
                        i32.add
                        local.set 6
                        i32.const 0
                        local.set 1
                        loop  ;; label = @11
                          local.get 2
                          local.get 1
                          i32.eq
                          br_if 10 (;@1;)
                          local.get 4
                          local.get 1
                          i32.add
                          local.get 6
                          local.get 1
                          i32.add
                          i32.load8_u
                          i32.store8
                          local.get 3
                          local.get 1
                          i32.const 1
                          i32.add
                          local.tee 1
                          i32.ne
                          br_if 0 (;@11;)
                        end
                        local.get 0
                        local.get 3
                        i32.store offset=4
                        local.get 0
                        local.get 4
                        i32.store
                      end
                      local.get 0
                      i32.load
                      local.set 1
                      local.get 0
                      i32.const 24
                      i32.add
                      i64.const 0
                      i64.store
                      local.get 0
                      i32.const 16
                      i32.add
                      i64.const 0
                      i64.store
                      local.get 0
                      i32.const 8
                      i32.add
                      i64.const 0
                      i64.store
                      local.get 0
                      i64.const 0
                      i64.store
                      local.get 1
                      local.get 3
                      local.get 0
                      call 1
                      local.get 0
                      i32.const 32
                      call 2
                      local.get 0
                      i32.const 48
                      i32.add
                      global.set 0
                      return
                    end
                    i32.const 12
                    i32.const 0
                    i32.const 1049156
                    call 7
                    unreachable
                  end
                  call 8
                  unreachable
                end
                local.get 4
                call 9
                unreachable
              end
              i32.const 12
              local.get 4
              i32.const 1049156
              call 7
              unreachable
            end
            local.get 3
            local.get 4
            i32.const 1049156
            call 10
            unreachable
          end
          local.get 5
          local.get 6
          i32.const 1049124
          call 10
          unreachable
        end
        local.get 6
        local.get 4
        i32.const 1049124
        call 7
        unreachable
      end
      local.get 3
      call 9
      unreachable
    end
    local.get 1
    local.get 2
    call 11
    unreachable)
  (func (;6;) (type 3) (param i32 i32)
    (local i32 i32 i32)
    global.get 0
    i32.const 32
    i32.sub
    local.tee 2
    global.set 0
    local.get 0
    i32.load offset=4
    local.tee 3
    i32.const 1
    i32.shl
    local.tee 4
    local.get 1
    local.get 4
    local.get 1
    i32.gt_u
    select
    local.tee 1
    i32.const 8
    local.get 1
    i32.const 8
    i32.gt_u
    select
    local.tee 1
    i32.const -1
    i32.xor
    i32.const 31
    i32.shr_u
    local.set 4
    block  ;; label = @1
      block  ;; label = @2
        local.get 3
        br_if 0 (;@2;)
        local.get 2
        i32.const 0
        i32.store offset=24
        br 1 (;@1;)
      end
      local.get 2
      local.get 3
      i32.store offset=28
      local.get 2
      i32.const 1
      i32.store offset=24
      local.get 2
      local.get 0
      i32.load
      i32.store offset=20
    end
    local.get 2
    i32.const 8
    i32.add
    local.get 4
    local.get 1
    local.get 2
    i32.const 20
    i32.add
    call 18
    local.get 2
    i32.load offset=12
    local.set 3
    block  ;; label = @1
      block  ;; label = @2
        local.get 2
        i32.load offset=8
        br_if 0 (;@2;)
        local.get 0
        local.get 1
        i32.store offset=4
        local.get 0
        local.get 3
        i32.store
        br 1 (;@1;)
      end
      local.get 3
      i32.const -2147483647
      i32.eq
      br_if 0 (;@1;)
      block  ;; label = @2
        local.get 3
        i32.eqz
        br_if 0 (;@2;)
        local.get 2
        i32.const 16
        i32.add
        i32.load
        call 9
        unreachable
      end
      call 8
      unreachable
    end
    local.get 2
    i32.const 32
    i32.add
    global.set 0)
  (func (;7;) (type 2) (param i32 i32 i32)
    (local i32)
    global.get 0
    i32.const 48
    i32.sub
    local.tee 3
    global.set 0
    local.get 3
    local.get 0
    i32.store
    local.get 3
    local.get 1
    i32.store offset=4
    local.get 3
    i32.const 8
    i32.add
    i32.const 12
    i32.add
    i64.const 2
    i64.store align=4
    local.get 3
    i32.const 32
    i32.add
    i32.const 12
    i32.add
    i32.const 1
    i32.store
    local.get 3
    i32.const 2
    i32.store offset=12
    local.get 3
    i32.const 1049020
    i32.store offset=8
    local.get 3
    i32.const 1
    i32.store offset=36
    local.get 3
    local.get 3
    i32.const 32
    i32.add
    i32.store offset=16
    local.get 3
    local.get 3
    i32.const 4
    i32.add
    i32.store offset=40
    local.get 3
    local.get 3
    i32.store offset=32
    local.get 3
    i32.const 8
    i32.add
    local.get 2
    call 13
    unreachable)
  (func (;8;) (type 5)
    (local i32)
    global.get 0
    i32.const 32
    i32.sub
    local.tee 0
    global.set 0
    local.get 0
    i32.const 20
    i32.add
    i64.const 0
    i64.store align=4
    local.get 0
    i32.const 1
    i32.store offset=12
    local.get 0
    i32.const 1048624
    i32.store offset=8
    local.get 0
    i32.const 1048700
    i32.store offset=16
    local.get 0
    i32.const 8
    i32.add
    i32.const 1048632
    call 13
    unreachable)
  (func (;9;) (type 4) (param i32)
    local.get 0
    call 12
    unreachable)
  (func (;10;) (type 2) (param i32 i32 i32)
    (local i32)
    global.get 0
    i32.const 48
    i32.sub
    local.tee 3
    global.set 0
    local.get 3
    local.get 0
    i32.store
    local.get 3
    local.get 1
    i32.store offset=4
    local.get 3
    i32.const 8
    i32.add
    i32.const 12
    i32.add
    i64.const 2
    i64.store align=4
    local.get 3
    i32.const 32
    i32.add
    i32.const 12
    i32.add
    i32.const 1
    i32.store
    local.get 3
    i32.const 2
    i32.store offset=12
    local.get 3
    i32.const 1049072
    i32.store offset=8
    local.get 3
    i32.const 1
    i32.store offset=36
    local.get 3
    local.get 3
    i32.const 32
    i32.add
    i32.store offset=16
    local.get 3
    local.get 3
    i32.const 4
    i32.add
    i32.store offset=40
    local.get 3
    local.get 3
    i32.store offset=32
    local.get 3
    i32.const 8
    i32.add
    local.get 2
    call 13
    unreachable)
  (func (;11;) (type 3) (param i32 i32)
    (local i32)
    global.get 0
    i32.const 48
    i32.sub
    local.tee 2
    global.set 0
    local.get 2
    local.get 1
    i32.store offset=4
    local.get 2
    local.get 0
    i32.store
    local.get 2
    i32.const 8
    i32.add
    i32.const 12
    i32.add
    i64.const 2
    i64.store align=4
    local.get 2
    i32.const 32
    i32.add
    i32.const 12
    i32.add
    i32.const 1
    i32.store
    local.get 2
    i32.const 2
    i32.store offset=12
    local.get 2
    i32.const 1048752
    i32.store offset=8
    local.get 2
    i32.const 1
    i32.store offset=36
    local.get 2
    local.get 2
    i32.const 32
    i32.add
    i32.store offset=16
    local.get 2
    local.get 2
    i32.store offset=40
    local.get 2
    local.get 2
    i32.const 4
    i32.add
    i32.store offset=32
    local.get 2
    i32.const 8
    i32.add
    i32.const 1049108
    call 13
    unreachable)
  (func (;12;) (type 4) (param i32)
    (local i32)
    global.get 0
    i32.const 48
    i32.sub
    local.tee 1
    global.set 0
    local.get 1
    local.get 0
    i32.store offset=12
    local.get 1
    i32.const 28
    i32.add
    i64.const 1
    i64.store align=4
    local.get 1
    i32.const 2
    i32.store offset=20
    local.get 1
    i32.const 1048684
    i32.store offset=16
    local.get 1
    i32.const 1
    i32.store offset=44
    local.get 1
    local.get 1
    i32.const 40
    i32.add
    i32.store offset=24
    local.get 1
    local.get 1
    i32.const 12
    i32.add
    i32.store offset=40
    local.get 1
    i32.const 16
    i32.add
    call 15
    unreachable)
  (func (;13;) (type 3) (param i32 i32)
    (local i32)
    global.get 0
    i32.const 16
    i32.sub
    local.tee 2
    global.set 0
    local.get 2
    i32.const 1048700
    call 16
    block  ;; label = @1
      local.get 2
      i64.load
      i64.const -4493808902380553279
      i64.xor
      local.get 2
      i32.const 8
      i32.add
      i64.load
      i64.const -163230743173927068
      i64.xor
      i64.or
      i64.const 0
      i64.ne
      br_if 0 (;@1;)
      local.get 2
      local.get 2
      call 2
    end
    i32.const -71
    call 3
    loop  ;; label = @1
      br 0 (;@1;)
    end)
  (func (;14;) (type 1) (param i32 i32) (result i32)
    (local i32 i32 i64 i64 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)
    global.get 0
    i32.const 48
    i32.sub
    local.tee 2
    global.set 0
    i32.const 39
    local.set 3
    block  ;; label = @1
      block  ;; label = @2
        local.get 0
        i64.load32_u
        local.tee 4
        i64.const 10000
        i64.ge_u
        br_if 0 (;@2;)
        local.get 4
        local.set 5
        br 1 (;@1;)
      end
      i32.const 39
      local.set 3
      loop  ;; label = @2
        local.get 2
        i32.const 9
        i32.add
        local.get 3
        i32.add
        local.tee 0
        i32.const -4
        i32.add
        local.get 4
        i64.const 10000
        i64.div_u
        local.tee 5
        i64.const 55536
        i64.mul
        local.get 4
        i64.add
        i32.wrap_i64
        local.tee 6
        i32.const 65535
        i32.and
        i32.const 100
        i32.div_u
        local.tee 7
        i32.const 1
        i32.shl
        i32.const 1048768
        i32.add
        i32.load16_u align=1
        i32.store16 align=1
        local.get 0
        i32.const -2
        i32.add
        local.get 7
        i32.const -100
        i32.mul
        local.get 6
        i32.add
        i32.const 65535
        i32.and
        i32.const 1
        i32.shl
        i32.const 1048768
        i32.add
        i32.load16_u align=1
        i32.store16 align=1
        local.get 3
        i32.const -4
        i32.add
        local.set 3
        local.get 4
        i64.const 99999999
        i64.gt_u
        local.set 0
        local.get 5
        local.set 4
        local.get 0
        br_if 0 (;@2;)
      end
    end
    block  ;; label = @1
      local.get 5
      i32.wrap_i64
      local.tee 0
      i32.const 99
      i32.le_u
      br_if 0 (;@1;)
      local.get 2
      i32.const 9
      i32.add
      local.get 3
      i32.const -2
      i32.add
      local.tee 3
      i32.add
      local.get 5
      i32.wrap_i64
      local.tee 6
      i32.const 65535
      i32.and
      i32.const 100
      i32.div_u
      local.tee 0
      i32.const -100
      i32.mul
      local.get 6
      i32.add
      i32.const 65535
      i32.and
      i32.const 1
      i32.shl
      i32.const 1048768
      i32.add
      i32.load16_u align=1
      i32.store16 align=1
    end
    block  ;; label = @1
      block  ;; label = @2
        local.get 0
        i32.const 10
        i32.lt_u
        br_if 0 (;@2;)
        local.get 2
        i32.const 9
        i32.add
        local.get 3
        i32.const -2
        i32.add
        local.tee 3
        i32.add
        local.get 0
        i32.const 1
        i32.shl
        i32.const 1048768
        i32.add
        i32.load16_u align=1
        i32.store16 align=1
        br 1 (;@1;)
      end
      local.get 2
      i32.const 9
      i32.add
      local.get 3
      i32.const -1
      i32.add
      local.tee 3
      i32.add
      local.get 0
      i32.const 48
      i32.add
      i32.store8
    end
    i32.const 39
    local.get 3
    i32.sub
    local.set 8
    i32.const 1
    local.set 7
    i32.const 43
    i32.const 1114112
    local.get 1
    i32.load offset=28
    local.tee 0
    i32.const 1
    i32.and
    local.tee 6
    select
    local.set 9
    local.get 0
    i32.const 29
    i32.shl
    i32.const 31
    i32.shr_s
    i32.const 1048700
    i32.and
    local.set 10
    local.get 2
    i32.const 9
    i32.add
    local.get 3
    i32.add
    local.set 11
    block  ;; label = @1
      block  ;; label = @2
        local.get 1
        i32.load
        br_if 0 (;@2;)
        local.get 1
        i32.load offset=20
        local.tee 3
        local.get 1
        i32.load offset=24
        local.tee 0
        local.get 9
        local.get 10
        call 17
        br_if 1 (;@1;)
        local.get 3
        local.get 11
        local.get 8
        local.get 0
        i32.load offset=12
        call_indirect (type 0)
        local.set 7
        br 1 (;@1;)
      end
      block  ;; label = @2
        local.get 1
        i32.load offset=4
        local.tee 12
        local.get 6
        local.get 8
        i32.add
        local.tee 7
        i32.gt_u
        br_if 0 (;@2;)
        i32.const 1
        local.set 7
        local.get 1
        i32.load offset=20
        local.tee 3
        local.get 1
        i32.load offset=24
        local.tee 0
        local.get 9
        local.get 10
        call 17
        br_if 1 (;@1;)
        local.get 3
        local.get 11
        local.get 8
        local.get 0
        i32.load offset=12
        call_indirect (type 0)
        local.set 7
        br 1 (;@1;)
      end
      block  ;; label = @2
        local.get 0
        i32.const 8
        i32.and
        i32.eqz
        br_if 0 (;@2;)
        local.get 1
        i32.load offset=16
        local.set 13
        local.get 1
        i32.const 48
        i32.store offset=16
        local.get 1
        i32.load8_u offset=32
        local.set 14
        i32.const 1
        local.set 7
        local.get 1
        i32.const 1
        i32.store8 offset=32
        local.get 1
        i32.load offset=20
        local.tee 0
        local.get 1
        i32.load offset=24
        local.tee 15
        local.get 9
        local.get 10
        call 17
        br_if 1 (;@1;)
        local.get 3
        local.get 12
        i32.add
        local.get 6
        i32.sub
        i32.const -38
        i32.add
        local.set 3
        block  ;; label = @3
          loop  ;; label = @4
            local.get 3
            i32.const -1
            i32.add
            local.tee 3
            i32.eqz
            br_if 1 (;@3;)
            local.get 0
            i32.const 48
            local.get 15
            i32.load offset=16
            call_indirect (type 1)
            i32.eqz
            br_if 0 (;@4;)
            br 3 (;@1;)
          end
        end
        local.get 0
        local.get 11
        local.get 8
        local.get 15
        i32.load offset=12
        call_indirect (type 0)
        br_if 1 (;@1;)
        local.get 1
        local.get 14
        i32.store8 offset=32
        local.get 1
        local.get 13
        i32.store offset=16
        i32.const 0
        local.set 7
        br 1 (;@1;)
      end
      local.get 12
      local.get 7
      i32.sub
      local.set 12
      block  ;; label = @2
        block  ;; label = @3
          block  ;; label = @4
            local.get 1
            i32.load8_u offset=32
            local.tee 3
            br_table 2 (;@2;) 0 (;@4;) 1 (;@3;) 0 (;@4;) 2 (;@2;)
          end
          local.get 12
          local.set 3
          i32.const 0
          local.set 12
          br 1 (;@2;)
        end
        local.get 12
        i32.const 1
        i32.shr_u
        local.set 3
        local.get 12
        i32.const 1
        i32.add
        i32.const 1
        i32.shr_u
        local.set 12
      end
      local.get 3
      i32.const 1
      i32.add
      local.set 3
      local.get 1
      i32.const 24
      i32.add
      i32.load
      local.set 0
      local.get 1
      i32.load offset=16
      local.set 15
      local.get 1
      i32.load offset=20
      local.set 6
      block  ;; label = @2
        loop  ;; label = @3
          local.get 3
          i32.const -1
          i32.add
          local.tee 3
          i32.eqz
          br_if 1 (;@2;)
          local.get 6
          local.get 15
          local.get 0
          i32.load offset=16
          call_indirect (type 1)
          i32.eqz
          br_if 0 (;@3;)
        end
        i32.const 1
        local.set 7
        br 1 (;@1;)
      end
      i32.const 1
      local.set 7
      local.get 6
      local.get 0
      local.get 9
      local.get 10
      call 17
      br_if 0 (;@1;)
      local.get 6
      local.get 11
      local.get 8
      local.get 0
      i32.load offset=12
      call_indirect (type 0)
      br_if 0 (;@1;)
      i32.const 0
      local.set 3
      loop  ;; label = @2
        block  ;; label = @3
          local.get 12
          local.get 3
          i32.ne
          br_if 0 (;@3;)
          local.get 12
          local.get 12
          i32.lt_u
          local.set 7
          br 2 (;@1;)
        end
        local.get 3
        i32.const 1
        i32.add
        local.set 3
        local.get 6
        local.get 15
        local.get 0
        i32.load offset=16
        call_indirect (type 1)
        i32.eqz
        br_if 0 (;@2;)
      end
      local.get 3
      i32.const -1
      i32.add
      local.get 12
      i32.lt_u
      local.set 7
    end
    local.get 2
    i32.const 48
    i32.add
    global.set 0
    local.get 7)
  (func (;15;) (type 4) (param i32)
    (local i32)
    global.get 0
    i32.const 16
    i32.sub
    local.tee 1
    global.set 0
    local.get 1
    i32.const 1048700
    call 16
    block  ;; label = @1
      local.get 1
      i64.load
      i64.const -4493808902380553279
      i64.xor
      local.get 1
      i32.const 8
      i32.add
      i64.load
      i64.const -163230743173927068
      i64.xor
      i64.or
      i64.const 0
      i64.ne
      br_if 0 (;@1;)
      local.get 1
      local.get 1
      call 2
    end
    i32.const -71
    call 3
    loop  ;; label = @1
      br 0 (;@1;)
    end)
  (func (;16;) (type 3) (param i32 i32)
    local.get 0
    i64.const 568815540544143123
    i64.store offset=8
    local.get 0
    i64.const 5657071353825360256
    i64.store)
  (func (;17;) (type 6) (param i32 i32 i32 i32) (result i32)
    (local i32)
    block  ;; label = @1
      block  ;; label = @2
        block  ;; label = @3
          local.get 2
          i32.const 1114112
          i32.eq
          br_if 0 (;@3;)
          i32.const 1
          local.set 4
          local.get 0
          local.get 2
          local.get 1
          i32.load offset=16
          call_indirect (type 1)
          br_if 1 (;@2;)
        end
        local.get 3
        br_if 1 (;@1;)
        i32.const 0
        local.set 4
      end
      local.get 4
      return
    end
    local.get 0
    local.get 3
    i32.const 0
    local.get 1
    i32.load offset=12
    call_indirect (type 0))
  (func (;18;) (type 7) (param i32 i32 i32 i32)
    (local i32)
    block  ;; label = @1
      block  ;; label = @2
        block  ;; label = @3
          local.get 1
          i32.eqz
          br_if 0 (;@3;)
          local.get 2
          i32.const -1
          i32.le_s
          br_if 1 (;@2;)
          block  ;; label = @4
            block  ;; label = @5
              block  ;; label = @6
                local.get 3
                i32.load offset=4
                i32.eqz
                br_if 0 (;@6;)
                block  ;; label = @7
                  local.get 3
                  i32.const 8
                  i32.add
                  i32.load
                  local.tee 4
                  br_if 0 (;@7;)
                  i32.const 0
                  i32.load8_u offset=1049172
                  drop
                  local.get 2
                  i32.const 65535
                  i32.add
                  i32.const 16
                  i32.shr_u
                  memory.grow
                  local.tee 1
                  i32.const -1
                  i32.ne
                  br_if 2 (;@5;)
                  br 6 (;@1;)
                end
                local.get 3
                i32.load
                local.set 3
                i32.const 0
                local.get 2
                i32.const 65535
                i32.add
                i32.const 16
                i32.shr_u
                memory.grow
                local.tee 1
                i32.const 16
                i32.shl
                local.get 1
                i32.const -1
                i32.eq
                select
                local.tee 1
                i32.eqz
                br_if 5 (;@1;)
                local.get 1
                local.get 3
                local.get 4
                call 21
                drop
                br 2 (;@4;)
              end
              i32.const 0
              i32.load8_u offset=1049172
              drop
              local.get 2
              i32.const 65535
              i32.add
              i32.const 16
              i32.shr_u
              memory.grow
              local.tee 1
              i32.const -1
              i32.eq
              br_if 4 (;@1;)
            end
            local.get 1
            i32.const 16
            i32.shl
            local.tee 1
            i32.eqz
            br_if 3 (;@1;)
          end
          local.get 0
          local.get 1
          i32.store offset=4
          local.get 0
          i32.const 8
          i32.add
          local.get 2
          i32.store
          local.get 0
          i32.const 0
          i32.store
          return
        end
        local.get 0
        i32.const 0
        i32.store offset=4
        local.get 0
        i32.const 8
        i32.add
        local.get 2
        i32.store
        local.get 0
        i32.const 1
        i32.store
        return
      end
      local.get 0
      i32.const 0
      i32.store offset=4
      local.get 0
      i32.const 1
      i32.store
      return
    end
    local.get 0
    i32.const 1
    i32.store offset=4
    local.get 0
    i32.const 8
    i32.add
    local.get 2
    i32.store
    local.get 0
    i32.const 1
    i32.store)
  (func (;19;) (type 0) (param i32 i32 i32) (result i32)
    (local i32 i32 i32 i32 i32 i32 i32 i32)
    block  ;; label = @1
      block  ;; label = @2
        local.get 2
        i32.const 16
        i32.ge_u
        br_if 0 (;@2;)
        local.get 0
        local.set 3
        br 1 (;@1;)
      end
      local.get 0
      i32.const 0
      local.get 0
      i32.sub
      i32.const 3
      i32.and
      local.tee 4
      i32.add
      local.set 5
      block  ;; label = @2
        local.get 4
        i32.eqz
        br_if 0 (;@2;)
        local.get 0
        local.set 3
        local.get 1
        local.set 6
        loop  ;; label = @3
          local.get 3
          local.get 6
          i32.load8_u
          i32.store8
          local.get 6
          i32.const 1
          i32.add
          local.set 6
          local.get 3
          i32.const 1
          i32.add
          local.tee 3
          local.get 5
          i32.lt_u
          br_if 0 (;@3;)
        end
      end
      local.get 5
      local.get 2
      local.get 4
      i32.sub
      local.tee 7
      i32.const -4
      i32.and
      local.tee 8
      i32.add
      local.set 3
      block  ;; label = @2
        block  ;; label = @3
          local.get 1
          local.get 4
          i32.add
          local.tee 9
          i32.const 3
          i32.and
          i32.eqz
          br_if 0 (;@3;)
          local.get 8
          i32.const 1
          i32.lt_s
          br_if 1 (;@2;)
          local.get 9
          i32.const 3
          i32.shl
          local.tee 6
          i32.const 24
          i32.and
          local.set 2
          local.get 9
          i32.const -4
          i32.and
          local.tee 10
          i32.const 4
          i32.add
          local.set 1
          i32.const 0
          local.get 6
          i32.sub
          i32.const 24
          i32.and
          local.set 4
          local.get 10
          i32.load
          local.set 6
          loop  ;; label = @4
            local.get 5
            local.get 6
            local.get 2
            i32.shr_u
            local.get 1
            i32.load
            local.tee 6
            local.get 4
            i32.shl
            i32.or
            i32.store
            local.get 1
            i32.const 4
            i32.add
            local.set 1
            local.get 5
            i32.const 4
            i32.add
            local.tee 5
            local.get 3
            i32.lt_u
            br_if 0 (;@4;)
            br 2 (;@2;)
          end
        end
        local.get 8
        i32.const 1
        i32.lt_s
        br_if 0 (;@2;)
        local.get 9
        local.set 1
        loop  ;; label = @3
          local.get 5
          local.get 1
          i32.load
          i32.store
          local.get 1
          i32.const 4
          i32.add
          local.set 1
          local.get 5
          i32.const 4
          i32.add
          local.tee 5
          local.get 3
          i32.lt_u
          br_if 0 (;@3;)
        end
      end
      local.get 7
      i32.const 3
      i32.and
      local.set 2
      local.get 9
      local.get 8
      i32.add
      local.set 1
    end
    block  ;; label = @1
      local.get 2
      i32.eqz
      br_if 0 (;@1;)
      local.get 3
      local.get 2
      i32.add
      local.set 5
      loop  ;; label = @2
        local.get 3
        local.get 1
        i32.load8_u
        i32.store8
        local.get 1
        i32.const 1
        i32.add
        local.set 1
        local.get 3
        i32.const 1
        i32.add
        local.tee 3
        local.get 5
        i32.lt_u
        br_if 0 (;@2;)
      end
    end
    local.get 0)
  (func (;20;) (type 0) (param i32 i32 i32) (result i32)
    (local i32 i32 i32)
    block  ;; label = @1
      block  ;; label = @2
        local.get 2
        i32.const 16
        i32.ge_u
        br_if 0 (;@2;)
        local.get 0
        local.set 3
        br 1 (;@1;)
      end
      local.get 0
      i32.const 0
      local.get 0
      i32.sub
      i32.const 3
      i32.and
      local.tee 4
      i32.add
      local.set 5
      block  ;; label = @2
        local.get 4
        i32.eqz
        br_if 0 (;@2;)
        local.get 0
        local.set 3
        loop  ;; label = @3
          local.get 3
          local.get 1
          i32.store8
          local.get 3
          i32.const 1
          i32.add
          local.tee 3
          local.get 5
          i32.lt_u
          br_if 0 (;@3;)
        end
      end
      local.get 5
      local.get 2
      local.get 4
      i32.sub
      local.tee 4
      i32.const -4
      i32.and
      local.tee 2
      i32.add
      local.set 3
      block  ;; label = @2
        local.get 2
        i32.const 1
        i32.lt_s
        br_if 0 (;@2;)
        local.get 1
        i32.const 255
        i32.and
        i32.const 16843009
        i32.mul
        local.set 2
        loop  ;; label = @3
          local.get 5
          local.get 2
          i32.store
          local.get 5
          i32.const 4
          i32.add
          local.tee 5
          local.get 3
          i32.lt_u
          br_if 0 (;@3;)
        end
      end
      local.get 4
      i32.const 3
      i32.and
      local.set 2
    end
    block  ;; label = @1
      local.get 2
      i32.eqz
      br_if 0 (;@1;)
      local.get 3
      local.get 2
      i32.add
      local.set 5
      loop  ;; label = @2
        local.get 3
        local.get 1
        i32.store8
        local.get 3
        i32.const 1
        i32.add
        local.tee 3
        local.get 5
        i32.lt_u
        br_if 0 (;@2;)
      end
    end
    local.get 0)
  (func (;21;) (type 0) (param i32 i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 2
    call 19)
  (func (;22;) (type 0) (param i32 i32 i32) (result i32)
    local.get 0
    local.get 1
    local.get 2
    call 20)
  (table (;0;) 2 2 funcref)
  (memory (;0;) 17)
  (global (;0;) (mut i32) (i32.const 1048576))
  (global (;1;) i32 (i32.const 1049173))
  (global (;2;) i32 (i32.const 1049184))
  (export "memory" (memory 0))
  (export "deploy" (func 4))
  (export "main" (func 5))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2))
  (elem (;0;) (i32.const 1) func 14)
  (data (;0;) (i32.const 1048576) "library/alloc/src/raw_vec.rscapacity overflow\00\00\00\1c\00\10\00\11\00\00\00\00\00\10\00\1c\00\00\00\17\02\00\00\05\00\00\00memory allocation of  bytes failed\00\00H\00\10\00\15\00\00\00]\00\10\00\0d\00\00\00index out of bounds: the len is  but the index is \00\00|\00\10\00 \00\00\00\9c\00\10\00\12\00\00\0000010203040506070809101112131415161718192021222324252627282930313233343536373839404142434445464748495051525354555657585960616263646566676869707172737475767778798081828384858687888990919293949596979899 out of range for slice of length range end index \00\00\aa\01\10\00\10\00\00\00\88\01\10\00\22\00\00\00slice index starts at  but ends at \00\cc\01\10\00\16\00\00\00\e2\01\10\00\0d\00\00\00codec/src/buffer.rs\00\00\02\10\00\13\00\00\00]\00\00\00\09\00\00\00\00\02\10\00\13\00\00\00o\00\00\00\15\00\00\00sdk/src/evm.rs\00\004\02\10\00\0e\00\00\00H\00\00\00\05\00\00\00"))
