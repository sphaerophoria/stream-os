#![allow(unused)]

struct MinMax {
    min: f32,
    max: f32,
}

fn find_min_max(data: &[f32]) -> MinMax {
    let mut min = f32::MAX;
    let mut max = f32::MIN;

    for value in data {
        let value = *value;

        if value > max {
            max = value;
        }

        if value < min {
            min = value;
        }
    }

    MinMax { min, max }
}

fn calculate_bucket_width(min: f32, max: f32, num_buckets: usize) -> f32 {
    (max - min) / num_buckets as f32
}

pub fn print_histogram(data: &[f32], num_buckets: usize) {
    let MinMax { min, max } = find_min_max(data);

    let bucket_width = calculate_bucket_width(min, max, num_buckets);

    let mut histogram = alloc::vec![0; num_buckets];
    for val in data {
        let mut bucket = ((val - min) / bucket_width) as usize;
        if bucket >= histogram.len() {
            bucket = histogram.len() - 1;
        }
        histogram[bucket] += 1;
    }

    let height = num_buckets * 4 / 3;
    let max_count = *histogram.iter().max().expect("No max value");

    for (bucket, count) in histogram.into_iter().enumerate() {
        print!("{:03}: ", bucket);
        for _ in 0..(count * height / max_count) {
            print!("#");
        }
        println!("");
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::testing::*;

    create_test!(test_min_max, {
        let data = [0.1, -0.5, 0.5, 100.0, -100.0];
        let min_max = find_min_max(&data);
        test_eq!(min_max.min, -100.0);
        test_eq!(min_max.max, 100.0);
        Ok(())
    });

    create_test!(test_calculate_bucket_width, {
        let val = calculate_bucket_width(0.1, 0.5, 4);
        unsafe {
            test_true!(core::intrinsics::fabsf32(val - 0.1) < 1e-9);
        }
        Ok(())
    });
}
