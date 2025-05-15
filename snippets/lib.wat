(module $snippets.wasm
  (type (;0;) (func (param i32 i32 i32 i32) (result i64)))
  (func $karatsuba_mul64_stack (type 0) (param i32 i32 i32 i32) (result i64)
    (local i32 i32 i32)
    local.get 3
    local.get 2
    i32.add
    local.get 1
    local.get 0
    i32.add
    i32.mul
    local.get 3
    local.get 1
    i32.mul
    local.get 2
    i32.const 65535
    i32.and
    local.tee 1
    local.get 0
    i32.const 65535
    i32.and
    local.tee 3
    i32.mul
    local.tee 4
    local.get 2
    i32.const 16
    i32.shr_u
    local.tee 5
    local.get 3
    i32.mul
    local.tee 3
    local.get 1
    local.get 0
    i32.const 16
    i32.shr_u
    local.tee 6
    i32.mul
    i32.add
    local.tee 0
    i32.const 16
    i32.shl
    i32.add
    local.tee 2
    i32.add
    i32.sub
    local.get 2
    local.get 4
    i32.lt_u
    i32.add
    local.get 5
    local.get 6
    i32.mul
    local.tee 1
    local.get 0
    i32.const 16
    i32.shr_u
    local.get 0
    local.get 3
    i32.lt_u
    i32.const 16
    i32.shl
    i32.or
    i32.add
    local.tee 0
    i32.add
    local.get 0
    local.get 1
    i32.lt_u
    i32.add
    i64.extend_i32_u
    i64.const 32
    i64.shl
    local.get 2
    i64.extend_i32_u
    i64.or)
  (func $add64_stack (type 0) (param i32 i32 i32 i32) (result i64)
    local.get 3
    local.get 1
    i32.add
    local.get 2
    local.get 0
    i32.add
    local.tee 1
    local.get 2
    i32.lt_u
    i32.add
    i64.extend_i32_u
    i64.const 32
    i64.shl
    local.get 1
    i64.extend_i32_u
    i64.or)
  (func $div64u_stack (type 0) (param i32 i32 i32 i32) (result i64)
    (local i32 i32 i32 i32 i32 i32 i32)
    block  ;; label = @1
      block  ;; label = @2
        local.get 3
        br_if 0 (;@2;)
        block  ;; label = @3
          local.get 2
          br_if 0 (;@3;)
        end
        block  ;; label = @3
          local.get 1
          br_if 0 (;@3;)
          i32.const 0
          local.set 4
          i32.const 0
          local.set 5
          local.get 0
          local.get 2
          i32.lt_u
          br_if 2 (;@1;)
        end
        i32.const 0
        local.set 4
        i32.const 63
        local.set 6
        i32.const 0
        local.set 7
        loop  ;; label = @3
          local.get 7
          i32.const 1
          i32.shl
          local.set 7
          block  ;; label = @4
            block  ;; label = @5
              local.get 6
              i32.const 31
              i32.gt_u
              br_if 0 (;@5;)
              local.get 1
              local.get 6
              i32.shr_u
              i32.const 1
              i32.and
              local.get 7
              i32.or
              local.tee 7
              local.get 2
              i32.lt_u
              br_if 1 (;@4;)
              local.get 7
              local.get 2
              i32.sub
              local.set 7
              local.get 4
              i32.const 1
              local.get 6
              i32.shl
              i32.or
              local.set 4
              br 1 (;@4;)
            end
            local.get 7
            local.get 2
            i32.lt_u
            br_if 0 (;@4;)
            local.get 7
            local.get 2
            i32.sub
            local.set 7
            i32.const 1
            local.get 6
            i32.shl
            local.get 4
            i32.or
            local.set 4
          end
          local.get 6
          i32.const -1
          i32.add
          local.tee 6
          i32.const -1
          i32.ne
          br_if 0 (;@3;)
        end
        i32.const 0
        local.set 5
        i32.const 63
        local.set 6
        i32.const 0
        local.set 1
        loop  ;; label = @3
          local.get 1
          i32.const 1
          i32.shl
          local.set 1
          block  ;; label = @4
            block  ;; label = @5
              local.get 6
              i32.const 31
              i32.gt_u
              br_if 0 (;@5;)
              local.get 0
              local.get 6
              i32.shr_u
              i32.const 1
              i32.and
              local.get 1
              i32.or
              local.tee 1
              local.get 2
              i32.lt_u
              br_if 1 (;@4;)
              local.get 1
              local.get 2
              i32.sub
              local.set 1
              local.get 5
              i32.const 1
              local.get 6
              i32.shl
              i32.or
              local.set 5
              br 1 (;@4;)
            end
            local.get 7
            local.get 6
            i32.shr_u
            i32.const 1
            i32.and
            local.get 1
            i32.or
            local.tee 1
            local.get 2
            i32.lt_u
            br_if 0 (;@4;)
            local.get 1
            local.get 2
            i32.sub
            local.set 1
            i32.const 1
            local.get 6
            i32.const 31
            i32.and
            i32.shl
            local.get 5
            i32.or
            local.set 5
          end
          local.get 6
          i32.const -1
          i32.add
          local.tee 6
          i32.const -1
          i32.ne
          br_if 0 (;@3;)
          br 2 (;@1;)
        end
      end
      i32.const 0
      local.set 4
      block  ;; label = @2
        local.get 1
        local.get 3
        i32.ge_u
        br_if 0 (;@2;)
        i32.const 0
        local.set 5
        br 1 (;@1;)
      end
      block  ;; label = @2
        local.get 0
        local.get 2
        i32.ge_u
        br_if 0 (;@2;)
        i32.const 0
        local.set 5
        local.get 1
        local.get 3
        i32.eq
        br_if 1 (;@1;)
      end
      i32.const 0
      local.set 7
      i32.const 64
      local.set 8
      i32.const 0
      local.set 9
      i32.const 0
      local.set 4
      i32.const 0
      local.set 5
      loop  ;; label = @2
        local.get 9
        i32.const 1
        i32.shl
        local.get 1
        i32.const 31
        i32.shr_u
        i32.or
        local.set 6
        block  ;; label = @3
          block  ;; label = @4
            local.get 7
            i32.const 1
            i32.shl
            local.get 9
            i32.const 31
            i32.shr_u
            i32.or
            local.tee 7
            local.get 3
            i32.gt_u
            br_if 0 (;@4;)
            i32.const 0
            local.set 10
            block  ;; label = @5
              local.get 7
              local.get 3
              i32.ne
              br_if 0 (;@5;)
              local.get 6
              local.get 2
              i32.ge_u
              br_if 1 (;@4;)
            end
            local.get 6
            local.set 9
            br 1 (;@3;)
          end
          local.get 7
          local.get 3
          i32.sub
          local.get 6
          local.get 2
          i32.lt_u
          i32.sub
          local.set 7
          local.get 6
          local.get 2
          i32.sub
          local.set 9
          i32.const 1
          local.set 10
        end
        local.get 1
        i32.const 1
        i32.shl
        local.get 0
        i32.const 31
        i32.shr_u
        i32.or
        local.set 1
        local.get 0
        i32.const 1
        i32.shl
        local.set 0
        local.get 4
        i32.const 1
        i32.shl
        local.get 5
        i32.const 31
        i32.shr_u
        i32.or
        local.set 4
        local.get 10
        local.get 5
        i32.const 1
        i32.shl
        i32.or
        local.set 5
        local.get 8
        i32.const -1
        i32.add
        local.tee 8
        br_if 0 (;@2;)
      end
    end
    local.get 4
    i64.extend_i32_u
    i64.const 32
    i64.shl
    local.get 5
    i64.extend_i32_u
    i64.or)
  (func $div64s_stack (type 0) (param i32 i32 i32 i32) (result i64)
    (local i32 i32 i32 i32 i32 i32 i32 i32 i32)
    block  ;; label = @1
      block  ;; label = @2
        local.get 1
        i32.const -1
        i32.le_s
        br_if 0 (;@2;)
        local.get 1
        local.set 4
        local.get 0
        local.set 5
        br 1 (;@1;)
      end
      i32.const 0
      local.get 0
      i32.sub
      local.set 5
      local.get 0
      i32.eqz
      local.get 1
      i32.const -1
      i32.xor
      i32.add
      local.set 4
    end
    block  ;; label = @1
      block  ;; label = @2
        local.get 3
        i32.const -1
        i32.le_s
        br_if 0 (;@2;)
        local.get 3
        local.set 6
        local.get 2
        local.set 7
        br 1 (;@1;)
      end
      i32.const 0
      local.get 2
      i32.sub
      local.set 7
      local.get 2
      i32.eqz
      local.get 3
      i32.const -1
      i32.xor
      i32.add
      local.set 6
    end
    i32.const 0
    local.set 2
    i32.const 64
    local.set 8
    i32.const 0
    local.set 9
    i32.const 0
    local.set 10
    i32.const 0
    local.set 11
    loop  ;; label = @1
      local.get 9
      i32.const 1
      i32.shl
      local.get 4
      i32.const 31
      i32.shr_u
      i32.or
      local.set 0
      block  ;; label = @2
        block  ;; label = @3
          local.get 2
          i32.const 1
          i32.shl
          local.get 9
          i32.const 31
          i32.shr_u
          i32.or
          local.tee 2
          local.get 6
          i32.gt_u
          br_if 0 (;@3;)
          i32.const 0
          local.set 12
          block  ;; label = @4
            local.get 2
            local.get 6
            i32.ne
            br_if 0 (;@4;)
            local.get 0
            local.get 7
            i32.ge_u
            br_if 1 (;@3;)
          end
          local.get 0
          local.set 9
          br 1 (;@2;)
        end
        local.get 2
        local.get 6
        i32.sub
        local.get 0
        local.get 7
        i32.lt_u
        i32.sub
        local.set 2
        local.get 0
        local.get 7
        i32.sub
        local.set 9
        i32.const 1
        local.set 12
      end
      local.get 4
      i32.const 1
      i32.shl
      local.get 5
      i32.const 31
      i32.shr_u
      i32.or
      local.set 4
      local.get 5
      i32.const 1
      i32.shl
      local.set 5
      local.get 10
      i32.const 1
      i32.shl
      local.get 11
      i32.const 31
      i32.shr_u
      i32.or
      local.set 10
      local.get 12
      local.get 11
      i32.const 1
      i32.shl
      i32.or
      local.set 11
      local.get 8
      i32.const -1
      i32.add
      local.tee 8
      br_if 0 (;@1;)
    end
    block  ;; label = @1
      block  ;; label = @2
        local.get 3
        local.get 1
        i32.xor
        i32.const -1
        i32.le_s
        br_if 0 (;@2;)
        local.get 11
        local.set 2
        br 1 (;@1;)
      end
      i32.const 0
      local.get 11
      i32.sub
      local.set 2
      local.get 11
      i32.eqz
      local.get 10
      i32.const -1
      i32.xor
      i32.add
      local.set 10
    end
    local.get 10
    i64.extend_i32_u
    i64.const 32
    i64.shl
    local.get 2
    i64.extend_i32_u
    i64.or)
  (func $sub64_stack (type 0) (param i32 i32 i32 i32) (result i64)
    local.get 1
    local.get 3
    i32.sub
    local.get 0
    local.get 2
    i32.lt_u
    i32.sub
    i64.extend_i32_u
    i64.const 32
    i64.shl
    local.get 0
    local.get 2
    i32.sub
    i64.extend_i32_u
    i64.or)
  (table (;0;) 1 1 funcref)
  (memory (;0;) 16)
  (global $__stack_pointer (mut i32) (i32.const 1048576))
  (global (;1;) i32 (i32.const 1048576))
  (global (;2;) i32 (i32.const 1048576))
  (export "memory" (memory 0))
  (export "karatsuba_mul64_stack" (func $karatsuba_mul64_stack))
  (export "add64_stack" (func $add64_stack))
  (export "div64u_stack" (func $div64u_stack))
  (export "div64s_stack" (func $div64s_stack))
  (export "sub64_stack" (func $sub64_stack))
  (export "__data_end" (global 1))
  (export "__heap_base" (global 2)))
