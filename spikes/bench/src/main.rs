use std::time::Instant;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[(self.next() % xs.len() as u64) as usize]
    }
}

fn p(mut v: Vec<f64>, q: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * q) as usize]
}

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1_000_000);
    let words = [
        "report", "invoice", "budget", "photo", "screenshot", "meeting", "notes", "draft", "final",
        "backup", "project", "design", "readme", "config", "main", "index", "test", "data", "image",
        "résumé", "café", "تقرير", "文档", "Terminal", "Chrome", "Visual Studio", "Quarterly",
    ];
    let exts = ["pdf", "docx", "png", "jpg", "txt", "rs", "ts", "json", "md", "xlsx", "exe", "lnk"];
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut idx = core::Index::default();
    if std::env::args().any(|a| a == "--real") {
        let root = std::env::args().skip_while(|a| a != "--real").nth(1).unwrap_or("C:\\".into());
        let t = Instant::now();
        let mut files = 0usize;
        for e in jwalk::WalkDir::new(&root).skip_hidden(false).follow_links(false).into_iter().flatten() {
            if let Some(name) = e.file_name().to_str() {
                idx.push(name);
                files += 1;
            }
        }
        println!("walked {root}: {files} entries in {:?}", t.elapsed());
    } else {
        // Realistic mix: mostly ASCII, ~3% names carry a non-ascii word.
        let ascii: Vec<&str> = words.iter().copied().filter(|w| w.is_ascii()).collect();
        let other: Vec<&str> = words.iter().copied().filter(|w| !w.is_ascii()).collect();
        for i in 0..n {
            let mut w = || if rng.next() % 100 < 1 { rng.pick(&other) } else { rng.pick(&ascii) };
            let name = format!("{}_{}-{}_{}.{}", w(), w(), w(), i, rng.pick(&exts));
            idx.push(&name);
        }
    }
    println!("{} names, {} threads", idx.len(), std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));

    println!("\n== cold single query (full scan) ==  p50 / p95 ms");
    for q in ["rep", "ter", "invoice pdf", "chrome", "mtgnts", "تقر", "zzzzqq"] {
        for (label, pf) in [("no-prefilter", false), ("prefilter   ", true)] {
            idx.matches(q, None, pf);
            let t: Vec<f64> = (0..30).map(|_| { let s = Instant::now(); std::hint::black_box(idx.matches(q, None, pf)); s.elapsed().as_secs_f64() * 1e3 }).collect();
            println!("{q:>12} {label}  {:6.1} / {:6.1}", p(t.clone(), 0.5), p(t, 0.95));
        }
    }

    println!("\n== top-20 bounded search (no full hit list) ==  p50 / p95 ms");
    for q in ["r", "re", "rep", "ter", "invoice pdf", "chrome", "mtgnts", "zzzzqq"] {
        idx.search_topk(q, 20);
        let t: Vec<f64> = (0..30).map(|_| { let s = Instant::now(); std::hint::black_box(idx.search_topk(q, 20)); s.elapsed().as_secs_f64() * 1e3 }).collect();
        println!("{q:>12}  {:6.1} / {:6.1}", p(t.clone(), 0.5), p(t, 0.95));
    }

    println!("\n== typing sequences: per-keystroke ms (full scan each time vs narrowing) ==");
    for word in ["report", "terminal", "chrome", "glint"] {
        let mut prev: Option<Vec<(u32, u32)>> = None;
        let mut line_full = String::new();
        let mut line_narrow = String::new();
        for end in 1..=word.len() {
            let q = &word[..end];
            let s = Instant::now();
            std::hint::black_box(idx.matches(q, None, true));
            line_full += &format!(" {:5.1}", s.elapsed().as_secs_f64() * 1e3);
            let s = Instant::now();
            let ids: Option<Vec<u32>> = prev.as_ref().map(|v| v.iter().map(|h| h.1).collect());
            let hits = idx.matches(q, ids.as_deref(), true);
            line_narrow += &format!(" {:5.1}", s.elapsed().as_secs_f64() * 1e3);
            prev = Some(hits);
        }
        println!("{word:>10}  full:{line_full}\n{:>10} narrow:{line_narrow}", "");
    }
}
