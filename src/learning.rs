use std::fs::{self, File};
use std::io::{self, BufRead, Write};
use std::path::Path;

pub fn force_threshold(path: &Path, value: u8) -> io::Result<()> {
    ensure_parent(path)?;
    fs::write(path, format!("{value}\n"))
}

pub fn random_learning(path: &Path) -> io::Result<()> {
    ensure_parent(path)?;
    let mut file = File::create(path)?;
    let mut seed = monotonic_seed();
    for _ in 0..25 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let value = ((seed >> 32) % 36) + 15;
        writeln!(file, "{value}")?;
    }
    Ok(())
}

pub fn clear_learning(learning_file: &Path, threshold_file: &Path) -> io::Result<()> {
    remove_if_exists(learning_file)?;
    remove_if_exists(threshold_file)
}

pub fn print_learning(path: &Path) -> io::Result<()> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            println!("There's no data yet");
            return Ok(());
        }
        Err(err) => return Err(err),
    };
    for (idx, line) in io::BufReader::new(file).lines().enumerate() {
        println!("line {}: {}", idx + 1, line?);
    }
    Ok(())
}

pub fn print_whitelist(path: &Path) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(text) => {
            print!("{text}");
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            println!("whitelist is empty");
            Ok(())
        }
        Err(err) => Err(err),
    }
}

pub fn export_learning(learning_file: &Path, threshold_file: &Path) -> io::Result<()> {
    fs::copy(learning_file, "/sdcard/battery_learning.dat")?;
    fs::copy(threshold_file, "/sdcard/battery_threshold.dat")?;
    Ok(())
}

pub fn import_learning(learning_file: &Path, threshold_file: &Path) -> io::Result<()> {
    ensure_parent(learning_file)?;
    ensure_parent(threshold_file)?;
    import_file(Path::new("/sdcard/battery_learning.dat"), learning_file)?;
    import_file(Path::new("/sdcard/battery_threshold.dat"), threshold_file)?;
    Ok(())
}

pub fn read_threshold(path: &Path, fallback: u8) -> u8 {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse::<u8>().ok())
        .filter(|value| *value <= 100)
        .unwrap_or(fallback)
}

pub fn learn_charge_point(
    learning_file: &Path,
    threshold_file: &Path,
    level: u8,
    min_points: usize,
    max_points: usize,
    min_threshold: u8,
    max_threshold: u8,
) -> io::Result<u8> {
    ensure_parent(learning_file)?;
    let mut points = read_learning_points(learning_file);
    points.push(level);
    if points.len() > max_points {
        points = points.split_off(points.len() - max_points);
    }
    let mut file = File::create(learning_file)?;
    for point in &points {
        writeln!(file, "{point}")?;
    }

    if points.len() < min_points {
        return Ok(read_threshold(threshold_file, min_threshold));
    }

    let old = read_threshold(threshold_file, min_threshold);
    let new = smooth_trimmed_threshold(&points, old, min_threshold, max_threshold);
    if new != old {
        force_threshold(threshold_file, new)?;
    }
    Ok(new)
}

fn smooth_trimmed_threshold(points: &[u8], old: u8, min: u8, max: u8) -> u8 {
    let mut sorted = points.to_vec();
    sorted.sort_unstable();
    let trim = sorted.len() / 5;
    let window = &sorted[trim..sorted.len().saturating_sub(trim)];
    let sum: u32 = window.iter().map(|value| u32::from(*value)).sum();
    let avg = if window.is_empty() {
        f32::from(old)
    } else {
        sum as f32 / window.len() as f32
    };
    let smoothed = (0.3 * avg) + (0.7 * f32::from(old));
    smoothed.round().clamp(f32::from(min), f32::from(max)) as u8
}

fn read_learning_points(path: &Path) -> Vec<u8> {
    fs::read_to_string(path)
        .ok()
        .map(|text| {
            text.lines()
                .filter_map(|line| line.trim().parse::<u8>().ok())
                .filter(|value| *value <= 100)
                .collect()
        })
        .unwrap_or_default()
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn import_file(src: &Path, dst: &Path) -> io::Result<()> {
    fs::copy(src, dst)?;
    remove_if_exists(src)
}

fn monotonic_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x5eed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_trimmed_and_smoothed() {
        let points = [15, 25, 26, 27, 28, 90];
        assert_eq!(smooth_trimmed_threshold(&points, 25, 25, 60), 26);
    }
}
