-- bench_fib.lua
local function fib(n)
    if n < 2 then return n end
    return fib(n - 1) + fib(n - 2)
end

local function bench(n, iters)
    -- warmup
    for _ = 1, 2 do fib(n) end

    local start = os.clock()
    local result = 0
    for _ = 1, iters do
        result = fib(n)
    end
    local elapsed = os.clock() - start

    print(string.format("fib(%d) = %d", n, result))
    print(string.format("%d iters in %.4fs (%.2f us/iter)",
        iters, elapsed, elapsed / iters * 1e6))
end

bench(30, 10)
