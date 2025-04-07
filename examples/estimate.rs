use card_est_array::{
    impls::{HyperLogLogBuilder, SliceEstimatorArray},
    traits::{Estimator, EstimatorArrayMut, EstimatorMut},
};

const N: usize = 1_000_000;
const ITERS: usize = 100_000_000;

fn main() {
    let logic = HyperLogLogBuilder::new(N)
        .log_2_num_reg(6)
        .build::<usize>()
        .unwrap();

    let mut array = SliceEstimatorArray::new(logic.clone(), 1);
    for i in 0..N {
        let mut estimator = array.get_estimator_mut(0);
        estimator.add(i);
    }

    let start = std::time::Instant::now();
    for _ in 0..ITERS {
        let _ = std::hint::black_box(array.get_estimator_mut(0).estimate());
    }
    let elapsed = start.elapsed();
    println!("{} ns/estimation", elapsed.as_nanos() as f64 / ITERS as f64);
}
