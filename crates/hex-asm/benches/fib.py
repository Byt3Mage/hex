# bench_fib.py
import time


def fib(n):
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)


def bench(n, iters, warmup=2):
    # warmup
    for _ in range(warmup):
        fib(n)

    start = time.perf_counter()
    result = 0
    for _ in range(iters):
        result = fib(n)
    elapsed = time.perf_counter() - start

    per_iter = elapsed / iters
    print(f"fib({n}) = {result}")
    print(f"{iters} iters in {elapsed:.4f}s ({per_iter * 1e6:.2f} µs/iter)")


if __name__ == "__main__":
    bench(30, 10)  # fib(30) = 832040, ~2.7M calls each
