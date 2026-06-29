use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use hex_asm::assemble;
use hex_vm::{Args, AsWord, Host, Program, RunOutcome, VM};

fn run_fib(vm: &mut VM, program: &Program, host: &mut impl Host, mem: &mut [u8], n: i64) -> i64 {
    vm.set_entry(program, 0, Args::new(&[n.into_word()]).unwrap()).unwrap();
    match hex_vm::run(vm, program, host, mem).unwrap() {
        RunOutcome::Completed => i64::from_word(vm.registers[0]),
        other => panic!("unexpected: {other:?}"),
    }
}

fn fib_bench(c: &mut Criterion) {
    let source = include_str!("fib.hxa");
    let program = assemble(source).unwrap();
    let mut memory = [];
    let mut vm = VM::new();

    c.bench_function("fib_recursive_30", |bch| {
        bch.iter(|| run_fib(&mut vm, &program, &mut (), &mut memory, black_box(30)));
    });
}

criterion_group!(benches, fib_bench);
criterion_main!(benches);
