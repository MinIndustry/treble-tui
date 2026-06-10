//! Euclidean rhythm distribution.

/// Produce a boolean hit pattern of length `steps` with `beats` hits.
pub fn euclidean(beats: u32, steps: u32, offset: u32) -> Vec<bool> {
    let steps = steps.max(1) as i32;
    let beats = beats.clamp(0, steps as u32) as i32;

    let mut pattern = vec![false; steps as usize];
    if beats == 0 {
        return pattern;
    }
    if beats >= steps {
        return vec![true; steps as usize];
    }

    let mut bucket = 0;
    for i in 0..steps {
        bucket += beats;
        if bucket >= steps {
            bucket -= steps;
            pattern[i as usize] = true;
        }
    }

    let offset = (offset as usize) % pattern.len();
    if offset > 0 {
        pattern.rotate_left(offset);
    }
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn euclidean_3_8() {
        let p = euclidean(3, 8, 0);
        assert_eq!(p.len(), 8);
        assert_eq!(p.iter().filter(|&&h| h).count(), 3);
    }

    #[test]
    fn euclidean_5_8() {
        let p = euclidean(5, 8, 0);
        assert_eq!(p.iter().filter(|&&h| h).count(), 5);
    }
}
