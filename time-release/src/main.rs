use std::{error::Error, hint::black_box};

use rand::Rng;

fn loops<const OUTER: usize, const INNER: usize>(divisor: i64, random: usize) -> i64 {
    let mut values = [0_i64; OUTER];

    for value in &mut values {
        for i in 0..INNER {
            *value += i as i64 % divisor;
        }
        *value += random as i64;
    }

    values[random]
}

fn main() -> Result<(), Box<dyn Error>> {
    let divisor = std::env::args()
        .nth(1)
        .map(|value| value.parse::<i64>())
        .transpose()?
        .unwrap_or(22);
    if divisor <= 0 {
        return Err("divisor must be positive".into());
    }

    let random = rand::rng().random_range(0..10_000);
    let result = loops::<10_000, 100_000>(black_box(divisor), random);
    println!("{result}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::loops;

    #[test]
    fn sums_remainders_and_random_value() {
        assert_eq!(loops::<4, 10>(3, 2), 11);
    }

    #[test]
    fn empty_inner_loop_keeps_random_value() {
        assert_eq!(loops::<4, 0>(22, 3), 3);
    }
}
