(module
  (type (;0;) (func))
  (type (;1;) (func (param i32 i32 i32)))
  (func (;0;) (type 0)
    (local i32 i64 i32 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64)
    global.get 0
    i32.const 96
    i32.sub
    local.tee 0
    global.set 0
    i32.const 32768
    i32.const 0
    i64.load offset=32768
    local.tee 1
    i32.wrap_i64
    local.tee 2
    i32.sub
    i64.load align=1
    local.set 3
    i32.const 32776
    local.get 2
    i32.sub
    i64.load align=1
    local.set 4
    i32.const 32784
    local.get 2
    i32.sub
    i64.load align=1
    local.set 5
    i32.const 32792
    local.get 2
    i32.sub
    i64.load align=1
    local.set 6
    i32.const 0
    local.get 1
    i64.const 32
    i64.shl
    i64.const -137438953472
    i64.add
    i64.const 32
    i64.shr_s
    local.tee 7
    i64.store offset=32768
    i32.const 32792
    local.get 7
    i32.wrap_i64
    local.tee 2
    i32.sub
    i64.load align=1
    local.tee 8
    i64.const 56
    i64.shl
    local.get 8
    i64.const 65280
    i64.and
    i64.const 40
    i64.shl
    i64.or
    local.get 8
    i64.const 16711680
    i64.and
    i64.const 24
    i64.shl
    local.get 8
    i64.const 4278190080
    i64.and
    i64.const 8
    i64.shl
    i64.or
    i64.or
    local.get 8
    i64.const 8
    i64.shr_u
    i64.const 4278190080
    i64.and
    local.get 8
    i64.const 24
    i64.shr_u
    i64.const 16711680
    i64.and
    i64.or
    local.get 8
    i64.const 40
    i64.shr_u
    i64.const 65280
    i64.and
    local.get 8
    i64.const 56
    i64.shr_u
    i64.or
    i64.or
    i64.or
    local.set 9
    i32.const 32784
    local.get 2
    i32.sub
    i64.load align=1
    local.set 10
    i32.const 32776
    local.get 2
    i32.sub
    i64.load align=1
    local.set 11
    block  ;; label = @1
      block  ;; label = @2
        block  ;; label = @3
          i32.const 32768
          local.get 2
          i32.sub
          local.tee 2
          i64.load align=1
          local.tee 12
          i64.const 0
          i64.ne
          br_if 0 (;@3;)
          local.get 11
          i64.const 0
          i64.ne
          br_if 0 (;@3;)
          local.get 10
          i64.const 0
          i64.ne
          br_if 0 (;@3;)
          i64.const 1
          local.set 1
          local.get 9
          i64.const 1
          i64.gt_u
          br_if 0 (;@3;)
          i64.const 0
          local.set 13
          local.get 8
          i64.const 0
          i64.ne
          br_if 1 (;@2;)
          local.get 3
          local.get 4
          i64.or
          local.get 5
          i64.or
          local.get 6
          i64.or
          i64.eqz
          i64.extend_i32_u
          local.set 1
          br 1 (;@2;)
        end
        local.get 6
        i64.const 56
        i64.shl
        local.get 6
        i64.const 65280
        i64.and
        i64.const 40
        i64.shl
        i64.or
        local.get 6
        i64.const 16711680
        i64.and
        i64.const 24
        i64.shl
        local.get 6
        i64.const 4278190080
        i64.and
        i64.const 8
        i64.shl
        i64.or
        i64.or
        local.get 6
        i64.const 8
        i64.shr_u
        i64.const 4278190080
        i64.and
        local.get 6
        i64.const 24
        i64.shr_u
        i64.const 16711680
        i64.and
        i64.or
        local.get 6
        i64.const 40
        i64.shr_u
        i64.const 65280
        i64.and
        local.get 6
        i64.const 56
        i64.shr_u
        i64.or
        i64.or
        i64.or
        local.set 14
        local.get 10
        i64.const 56
        i64.shl
        local.get 10
        i64.const 65280
        i64.and
        i64.const 40
        i64.shl
        i64.or
        local.get 10
        i64.const 16711680
        i64.and
        i64.const 24
        i64.shl
        local.get 10
        i64.const 4278190080
        i64.and
        i64.const 8
        i64.shl
        i64.or
        i64.or
        local.get 10
        i64.const 8
        i64.shr_u
        i64.const 4278190080
        i64.and
        local.get 10
        i64.const 24
        i64.shr_u
        i64.const 16711680
        i64.and
        i64.or
        local.get 10
        i64.const 40
        i64.shr_u
        i64.const 65280
        i64.and
        local.get 10
        i64.const 56
        i64.shr_u
        i64.or
        i64.or
        i64.or
        local.set 10
        local.get 11
        i64.const 56
        i64.shl
        local.get 11
        i64.const 65280
        i64.and
        i64.const 40
        i64.shl
        i64.or
        local.get 11
        i64.const 16711680
        i64.and
        i64.const 24
        i64.shl
        local.get 11
        i64.const 4278190080
        i64.and
        i64.const 8
        i64.shl
        i64.or
        i64.or
        local.get 11
        i64.const 8
        i64.shr_u
        i64.const 4278190080
        i64.and
        local.get 11
        i64.const 24
        i64.shr_u
        i64.const 16711680
        i64.and
        i64.or
        local.get 11
        i64.const 40
        i64.shr_u
        i64.const 65280
        i64.and
        local.get 11
        i64.const 56
        i64.shr_u
        i64.or
        i64.or
        i64.or
        local.set 15
        local.get 12
        i64.const 56
        i64.shl
        local.get 12
        i64.const 65280
        i64.and
        i64.const 40
        i64.shl
        i64.or
        local.get 12
        i64.const 16711680
        i64.and
        i64.const 24
        i64.shl
        local.get 12
        i64.const 4278190080
        i64.and
        i64.const 8
        i64.shl
        i64.or
        i64.or
        local.get 12
        i64.const 8
        i64.shr_u
        i64.const 4278190080
        i64.and
        local.get 12
        i64.const 24
        i64.shr_u
        i64.const 16711680
        i64.and
        i64.or
        local.get 12
        i64.const 40
        i64.shr_u
        i64.const 65280
        i64.and
        local.get 12
        i64.const 56
        i64.shr_u
        i64.or
        i64.or
        i64.or
        local.set 12
        block  ;; label = @3
          local.get 3
          i64.const 0
          i64.ne
          br_if 0 (;@3;)
          local.get 4
          i64.const 0
          i64.ne
          br_if 0 (;@3;)
          local.get 5
          i64.const 0
          i64.ne
          br_if 0 (;@3;)
          i64.const 1
          local.set 1
          local.get 14
          i64.const 1
          i64.gt_u
          br_if 0 (;@3;)
          i64.const 0
          local.set 13
          i64.const 0
          local.set 11
          i64.const 0
          local.set 8
          local.get 6
          i64.const 72057594037927936
          i64.ne
          br_if 2 (;@1;)
          local.get 12
          local.set 13
          local.get 15
          local.set 11
          local.get 10
          local.set 8
          local.get 9
          local.set 1
          br 2 (;@1;)
        end
        local.get 5
        i64.const 56
        i64.shl
        local.get 5
        i64.const 65280
        i64.and
        i64.const 40
        i64.shl
        i64.or
        local.get 5
        i64.const 16711680
        i64.and
        i64.const 24
        i64.shl
        local.get 5
        i64.const 4278190080
        i64.and
        i64.const 8
        i64.shl
        i64.or
        i64.or
        local.get 5
        i64.const 8
        i64.shr_u
        i64.const 4278190080
        i64.and
        local.get 5
        i64.const 24
        i64.shr_u
        i64.const 16711680
        i64.and
        i64.or
        local.get 5
        i64.const 40
        i64.shr_u
        i64.const 65280
        i64.and
        local.get 5
        i64.const 56
        i64.shr_u
        i64.or
        i64.or
        i64.or
        local.set 5
        local.get 4
        i64.const 56
        i64.shl
        local.get 4
        i64.const 65280
        i64.and
        i64.const 40
        i64.shl
        i64.or
        local.get 4
        i64.const 16711680
        i64.and
        i64.const 24
        i64.shl
        local.get 4
        i64.const 4278190080
        i64.and
        i64.const 8
        i64.shl
        i64.or
        i64.or
        local.get 4
        i64.const 8
        i64.shr_u
        i64.const 4278190080
        i64.and
        local.get 4
        i64.const 24
        i64.shr_u
        i64.const 16711680
        i64.and
        i64.or
        local.get 4
        i64.const 40
        i64.shr_u
        i64.const 65280
        i64.and
        local.get 4
        i64.const 56
        i64.shr_u
        i64.or
        i64.or
        i64.or
        local.set 16
        local.get 3
        i64.const 56
        i64.shl
        local.get 3
        i64.const 65280
        i64.and
        i64.const 40
        i64.shl
        i64.or
        local.get 3
        i64.const 16711680
        i64.and
        i64.const 24
        i64.shl
        local.get 3
        i64.const 4278190080
        i64.and
        i64.const 8
        i64.shl
        i64.or
        i64.or
        local.get 3
        i64.const 8
        i64.shr_u
        i64.const 4278190080
        i64.and
        local.get 3
        i64.const 24
        i64.shr_u
        i64.const 16711680
        i64.and
        i64.or
        local.get 3
        i64.const 40
        i64.shr_u
        i64.const 65280
        i64.and
        local.get 3
        i64.const 56
        i64.shr_u
        i64.or
        i64.or
        i64.or
        local.set 6
        i64.const 0
        local.set 17
        i64.const 0
        local.set 18
        i64.const 0
        local.set 3
        i64.const 1
        local.set 19
        i64.const 0
        local.set 20
        i64.const 0
        local.set 21
        i64.const 0
        local.set 22
        i64.const 1
        local.set 23
        loop  ;; label = @3
          local.get 19
          local.set 1
          local.get 3
          local.set 8
          local.get 18
          local.set 11
          local.get 17
          local.set 13
          local.get 5
          local.set 4
          local.get 16
          local.set 5
          block  ;; label = @4
            block  ;; label = @5
              local.get 14
              i64.const 1
              i64.and
              i64.eqz
              i32.eqz
              br_if 0 (;@5;)
              local.get 13
              local.set 17
              local.get 11
              local.set 18
              local.get 8
              local.set 3
              local.get 1
              local.set 19
              br 1 (;@4;)
            end
            local.get 0
            local.get 20
            i64.store offset=56
            local.get 0
            local.get 21
            i64.store offset=48
            local.get 0
            local.get 22
            i64.store offset=40
            local.get 0
            local.get 23
            i64.store offset=32
            local.get 0
            local.get 12
            i64.store offset=88
            local.get 0
            local.get 15
            i64.store offset=80
            local.get 0
            local.get 10
            i64.store offset=72
            local.get 0
            local.get 9
            i64.store offset=64
            local.get 0
            local.get 0
            i32.const 32
            i32.add
            local.get 0
            i32.const 64
            i32.add
            call 1
            local.get 0
            i64.load offset=8
            local.set 3
            local.get 0
            i64.load offset=16
            local.set 18
            local.get 0
            i64.load offset=24
            local.set 17
            block  ;; label = @5
              local.get 0
              i64.load
              local.tee 19
              local.get 1
              i64.ne
              br_if 0 (;@5;)
              local.get 3
              local.get 8
              i64.ne
              br_if 0 (;@5;)
              local.get 18
              local.get 11
              i64.ne
              br_if 0 (;@5;)
              local.get 17
              local.set 20
              local.get 18
              local.set 21
              local.get 3
              local.set 22
              local.get 19
              local.set 23
              local.get 17
              local.get 13
              i64.eq
              br_if 4 (;@1;)
              br 1 (;@4;)
            end
            local.get 17
            local.set 20
            local.get 18
            local.set 21
            local.get 3
            local.set 22
            local.get 19
            local.set 23
          end
          local.get 6
          i64.const 63
          i64.shl
          local.get 5
          i64.const 1
          i64.shr_u
          i64.or
          local.set 16
          local.get 5
          i64.const 63
          i64.shl
          local.get 4
          i64.const 1
          i64.shr_u
          i64.or
          local.set 5
          block  ;; label = @4
            local.get 4
            i64.const 63
            i64.shl
            local.get 14
            i64.const 1
            i64.shr_u
            i64.or
            local.tee 14
            i64.const 0
            i64.ne
            br_if 0 (;@4;)
            local.get 5
            i64.const 0
            i64.ne
            br_if 0 (;@4;)
            local.get 16
            i64.const 0
            i64.ne
            br_if 0 (;@4;)
            local.get 6
            i64.const 2
            i64.ge_u
            br_if 0 (;@4;)
            local.get 20
            local.set 13
            local.get 21
            local.set 11
            local.get 22
            local.set 8
            local.get 23
            local.set 1
            br 3 (;@1;)
          end
          local.get 0
          local.get 12
          i64.store offset=56
          local.get 0
          local.get 15
          i64.store offset=48
          local.get 0
          local.get 10
          i64.store offset=40
          local.get 0
          local.get 9
          i64.store offset=32
          local.get 0
          local.get 12
          i64.store offset=88
          local.get 0
          local.get 15
          i64.store offset=80
          local.get 0
          local.get 10
          i64.store offset=72
          local.get 0
          local.get 9
          i64.store offset=64
          local.get 6
          i64.const 1
          i64.shr_u
          local.set 6
          local.get 0
          local.get 0
          i32.const 32
          i32.add
          local.get 0
          i32.const 64
          i32.add
          call 1
          local.get 0
          i64.load
          local.set 9
          local.get 0
          i64.load offset=8
          local.set 10
          local.get 0
          i64.load offset=16
          local.set 15
          local.get 0
          i64.load offset=24
          local.set 12
          br 0 (;@3;)
        end
      end
      i64.const 0
      local.set 11
      i64.const 0
      local.set 8
    end
    i32.const 0
    local.get 7
    i64.store offset=32768
    local.get 2
    local.get 1
    i64.const 56
    i64.shl
    local.get 1
    i64.const 65280
    i64.and
    i64.const 40
    i64.shl
    i64.or
    local.get 1
    i64.const 16711680
    i64.and
    i64.const 24
    i64.shl
    local.get 1
    i64.const 4278190080
    i64.and
    i64.const 8
    i64.shl
    i64.or
    i64.or
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
    i64.or
    i64.store offset=24 align=1
    local.get 2
    local.get 8
    i64.const 56
    i64.shl
    local.get 8
    i64.const 65280
    i64.and
    i64.const 40
    i64.shl
    i64.or
    local.get 8
    i64.const 16711680
    i64.and
    i64.const 24
    i64.shl
    local.get 8
    i64.const 4278190080
    i64.and
    i64.const 8
    i64.shl
    i64.or
    i64.or
    local.get 8
    i64.const 8
    i64.shr_u
    i64.const 4278190080
    i64.and
    local.get 8
    i64.const 24
    i64.shr_u
    i64.const 16711680
    i64.and
    i64.or
    local.get 8
    i64.const 40
    i64.shr_u
    i64.const 65280
    i64.and
    local.get 8
    i64.const 56
    i64.shr_u
    i64.or
    i64.or
    i64.or
    i64.store offset=16 align=1
    local.get 2
    local.get 11
    i64.const 56
    i64.shl
    local.get 11
    i64.const 65280
    i64.and
    i64.const 40
    i64.shl
    i64.or
    local.get 11
    i64.const 16711680
    i64.and
    i64.const 24
    i64.shl
    local.get 11
    i64.const 4278190080
    i64.and
    i64.const 8
    i64.shl
    i64.or
    i64.or
    local.get 11
    i64.const 8
    i64.shr_u
    i64.const 4278190080
    i64.and
    local.get 11
    i64.const 24
    i64.shr_u
    i64.const 16711680
    i64.and
    i64.or
    local.get 11
    i64.const 40
    i64.shr_u
    i64.const 65280
    i64.and
    local.get 11
    i64.const 56
    i64.shr_u
    i64.or
    i64.or
    i64.or
    i64.store offset=8 align=1
    local.get 2
    local.get 13
    i64.const 56
    i64.shl
    local.get 13
    i64.const 65280
    i64.and
    i64.const 40
    i64.shl
    i64.or
    local.get 13
    i64.const 16711680
    i64.and
    i64.const 24
    i64.shl
    local.get 13
    i64.const 4278190080
    i64.and
    i64.const 8
    i64.shl
    i64.or
    i64.or
    local.get 13
    i64.const 8
    i64.shr_u
    i64.const 4278190080
    i64.and
    local.get 13
    i64.const 24
    i64.shr_u
    i64.const 16711680
    i64.and
    i64.or
    local.get 13
    i64.const 40
    i64.shr_u
    i64.const 65280
    i64.and
    local.get 13
    i64.const 56
    i64.shr_u
    i64.or
    i64.or
    i64.or
    i64.store align=1
    local.get 0
    i32.const 96
    i32.add
    global.set 0)
  (func (;1;) (type 1) (param i32 i32 i32)
    (local i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64)
    local.get 0
    local.get 1
    i64.load
    local.tee 3
    i64.const 4294967295
    i64.and
    local.tee 4
    local.get 2
    i64.load
    local.tee 5
    i64.const 4294967295
    i64.and
    local.tee 6
    i64.mul
    local.tee 7
    local.get 4
    local.get 5
    i64.const 32
    i64.shr_u
    local.tee 8
    i64.mul
    local.tee 9
    local.get 3
    i64.const 32
    i64.shr_u
    local.tee 10
    local.get 6
    i64.mul
    i64.add
    local.tee 11
    i64.const 32
    i64.shl
    i64.add
    local.tee 12
    i64.store
    local.get 0
    local.get 4
    local.get 2
    i64.load offset=8
    local.tee 13
    i64.const 4294967295
    i64.and
    local.tee 14
    i64.mul
    local.tee 15
    local.get 4
    local.get 13
    i64.const 32
    i64.shr_u
    local.tee 16
    i64.mul
    local.tee 17
    local.get 10
    local.get 14
    i64.mul
    i64.add
    local.tee 18
    i64.const 32
    i64.shl
    i64.add
    local.tee 19
    local.get 1
    i64.load offset=8
    local.tee 20
    i64.const 4294967295
    i64.and
    local.tee 21
    local.get 6
    i64.mul
    local.tee 22
    local.get 21
    local.get 8
    i64.mul
    local.tee 23
    local.get 20
    i64.const 32
    i64.shr_u
    local.tee 24
    local.get 6
    i64.mul
    i64.add
    local.tee 25
    i64.const 32
    i64.shl
    i64.add
    local.tee 26
    local.get 11
    i64.const 32
    i64.shr_u
    local.get 10
    local.get 8
    i64.mul
    i64.add
    local.get 11
    local.get 9
    i64.lt_u
    i64.extend_i32_u
    i64.const 32
    i64.shl
    i64.add
    local.get 12
    local.get 7
    i64.lt_u
    i64.extend_i32_u
    i64.add
    i64.add
    local.tee 12
    i64.add
    local.tee 9
    i64.store offset=8
    local.get 0
    local.get 4
    local.get 2
    i64.load offset=16
    local.tee 11
    i64.const 4294967295
    i64.and
    local.tee 7
    i64.mul
    local.tee 27
    local.get 4
    local.get 11
    i64.const 32
    i64.shr_u
    local.tee 28
    i64.mul
    local.tee 29
    local.get 10
    local.get 7
    i64.mul
    i64.add
    local.tee 4
    i64.const 32
    i64.shl
    i64.add
    local.tee 7
    local.get 21
    local.get 14
    i64.mul
    local.tee 30
    local.get 21
    local.get 16
    i64.mul
    local.tee 31
    local.get 24
    local.get 14
    i64.mul
    i64.add
    local.tee 14
    i64.const 32
    i64.shl
    i64.add
    local.tee 21
    local.get 18
    i64.const 32
    i64.shr_u
    local.get 10
    local.get 16
    i64.mul
    i64.add
    local.get 18
    local.get 17
    i64.lt_u
    i64.extend_i32_u
    i64.const 32
    i64.shl
    i64.add
    local.get 19
    local.get 15
    i64.lt_u
    i64.extend_i32_u
    i64.add
    local.tee 15
    local.get 9
    local.get 19
    i64.lt_u
    i64.extend_i32_u
    i64.add
    local.tee 18
    local.get 1
    i64.load offset=16
    local.tee 19
    i64.const 4294967295
    i64.and
    local.tee 9
    local.get 6
    i64.mul
    local.tee 17
    local.get 9
    local.get 8
    i64.mul
    local.tee 32
    local.get 19
    i64.const 32
    i64.shr_u
    local.tee 33
    local.get 6
    i64.mul
    i64.add
    local.tee 6
    i64.const 32
    i64.shl
    i64.add
    local.tee 9
    local.get 25
    i64.const 32
    i64.shr_u
    local.get 24
    local.get 8
    i64.mul
    i64.add
    local.get 25
    local.get 23
    i64.lt_u
    i64.extend_i32_u
    i64.const 32
    i64.shl
    i64.add
    local.get 26
    local.get 22
    i64.lt_u
    i64.extend_i32_u
    i64.add
    local.tee 25
    local.get 12
    local.get 26
    i64.lt_u
    i64.extend_i32_u
    i64.add
    local.tee 26
    i64.add
    local.tee 12
    i64.add
    local.tee 22
    i64.add
    local.tee 23
    i64.add
    local.tee 34
    i64.store offset=16
    local.get 0
    local.get 3
    local.get 2
    i64.load offset=24
    i64.mul
    local.get 11
    local.get 20
    i64.mul
    local.get 4
    i64.const 32
    i64.shr_u
    local.get 10
    local.get 28
    i64.mul
    i64.add
    local.get 4
    local.get 29
    i64.lt_u
    i64.extend_i32_u
    i64.const 32
    i64.shl
    i64.add
    local.get 7
    local.get 27
    i64.lt_u
    i64.extend_i32_u
    i64.add
    local.get 34
    local.get 7
    i64.lt_u
    i64.extend_i32_u
    i64.add
    local.get 13
    local.get 19
    i64.mul
    local.get 14
    i64.const 32
    i64.shr_u
    local.get 24
    local.get 16
    i64.mul
    i64.add
    local.get 14
    local.get 31
    i64.lt_u
    i64.extend_i32_u
    i64.const 32
    i64.shl
    i64.add
    local.get 21
    local.get 30
    i64.lt_u
    i64.extend_i32_u
    i64.add
    local.get 22
    local.get 18
    i64.lt_u
    i64.extend_i32_u
    local.get 18
    local.get 15
    i64.lt_u
    i64.extend_i32_u
    i64.add
    local.get 23
    local.get 21
    i64.lt_u
    i64.extend_i32_u
    i64.add
    i64.add
    local.get 5
    local.get 1
    i64.load offset=24
    i64.mul
    local.get 6
    i64.const 32
    i64.shr_u
    local.get 33
    local.get 8
    i64.mul
    i64.add
    local.get 6
    local.get 32
    i64.lt_u
    i64.extend_i32_u
    i64.const 32
    i64.shl
    i64.add
    local.get 9
    local.get 17
    i64.lt_u
    i64.extend_i32_u
    i64.add
    local.get 26
    local.get 25
    i64.lt_u
    i64.extend_i32_u
    local.get 12
    local.get 9
    i64.lt_u
    i64.extend_i32_u
    i64.add
    i64.add
    i64.add
    i64.add
    i64.add
    i64.add
    i64.add
    i64.add
    i64.store offset=24)
  (memory (;0;) 16)
  (global (;0;) (mut i32) (i32.const 1048576))
  (global (;1;) i32 (i32.const 1048576))
  (global (;2;) i32 (i32.const 1048576))
  (export "memory" (memory 0))
  (export "arithmetic_exp" (func 0))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2)))