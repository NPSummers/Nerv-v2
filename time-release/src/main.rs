use std::{error::Error, hint::black_box};

use rand::Rng;

fn loops<const OUTER: usize, const INNER: usize>(divisor: i64, random: usize) -> i64 {
    let mut values = [0_i64; OUTER];

    for value in &mut values {
        // Prevent hoisting one identical inner sum outside all 10,000 rows.
        // This barrier executes per outer row, not per inner iteration.
        let divisor = black_box(divisor);
        for i in 0..INNER {
            *value += i as i64 % divisor;
        }
        *value += random as i64;
    }

    // Make all rows observable, not just the randomly selected result element.
    black_box(values)[random]
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

    #[test]
    fn default_inner_sum_matches_benchmark_output_validation() {
        assert_eq!(loops::<1, 100_000>(22, 0), 1_049_940);
    }
}
