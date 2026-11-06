//! Search benchmark. Usage:
//!   bench [N] [--real [ROOT]] [--gate MS]
//! Run with RAYON_NUM_THREADS=4 to model a mid-range machine. With --gate, exits 1 if any
//! fast-path query exceeds MS at p95.
use glint_core::Index;
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

fn pct(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * q) as usize]
}

fn time(mut f: impl FnMut() -> usize, runs: usize) -> (f64, f64, usize) {
    let hits = f(); // warm-up
    let mut t: Vec<f64> = (0..runs)
        .map(|_| {
            let s = Instant::now();
            std::hint::black_box(f());
            s.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    (pct(&mut t.clone(), 0.5), pct(&mut t, 0.95), hits)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|a| a == name);
    let n: usize = args
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000_000);
    let gate: Option<f64> = flag("--gate")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok());

    let path_arg = |name: &str| {
        flag(name)
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from)
    };
    let mut ix = Index::default();
    if let Some(p) = path_arg("--load") {
        let t = Instant::now();
        ix = Index::open(&p).expect("open snapshot");
        println!("mapped {} in {:?}", p.display(), t.elapsed());
    } else if let Some(p) = path_arg("--load-heap") {
        let t = Instant::now();
        ix = Index::read(&p).expect("read snapshot");
        println!("read into heap {} in {:?}", p.display(), t.elapsed());
    } else if let Some(i) = flag("--real") {
        let root = args
            .get(i + 1)
            .filter(|a| !a.starts_with("--"))
            .cloned()
            .unwrap_or("C:\\".into());
        let t = Instant::now();
        for e in jwalk::WalkDir::new(&root)
            .skip_hidden(false)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if let Some(name) = e.file_name().to_str() {
                ix.push(name);
            }
        }
        println!("walked {root}: {} entries in {:?}", ix.len(), t.elapsed());
    } else {
        let words = [
            "report",
            "invoice",
            "budget",
            "photo",
            "screenshot",
            "meeting",
            "notes",
            "draft",
            "final",
            "backup",
            "project",
            "design",
            "readme",
            "config",
            "main",
            "index",
            "test",
            "data",
            "image",
            "Terminal",
            "Chrome",
            "Visual Studio",
            "Quarterly",
        ];
        let other = ["résumé", "café", "تقرير", "文档"];
        let exts = [
            "pdf", "docx", "png", "jpg", "txt", "rs", "ts", "json", "md", "xlsx", "exe", "lnk",
        ];
        let mut rng = Rng(0x9E3779B97F4A7C15);
        for i in 0..n {
            let mut w = || {
                if rng.next() % 100 < 1 {
                    rng.pick(&other)
                } else {
                    rng.pick(&words)
                }
            };
            let name = format!("{}_{}-{}_{}.{}", w(), w(), w(), i, rng.pick(&exts));
            ix.push(&name);
        }
    }
    println!(
        "{} names, {:.1} MB heap + {:.1} MB mapped, {} rayon threads",
        ix.len(),
        ix.heap_bytes() as f64 / 1e6,
        ix.mapped_bytes() as f64 / 1e6,
        rayon_threads()
    );
    if let Some(p) = path_arg("--save") {
        let t = Instant::now();
        ix.save(&p).expect("save snapshot");
        println!(
            "saved {} ({:.1} MB) in {:?}",
            p.display(),
            std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) as f64 / 1e6,
            t.elapsed()
        );
    }

    println!("\nquery           fast p50 / p95 ms   hits    brute-force p50 / p95 ms");
    let mut worst: f64 = 0.0;
    let skip = flag("--no-queries").is_some();
    for q in [
        "r",
        "re",
        "rep",
        "ter",
        "invoice pdf",
        "chrome",
        "visual studio",
        "résu",
        "تقر",
        "zzzzqq",
    ]
    .into_iter()
    .filter(|_| !skip)
    {
        let (p50, p95, hits) = time(|| ix.search(q, 20).len(), 40);
        let (b50, b95, _) = if flag("--no-brute").is_some() {
            (0.0, 0.0, 0)
        } else {
            time(|| ix.search_fuzzy(q, 20).len(), 10)
        };
        worst = worst.max(p95);
        println!("{q:<14} {p50:8.1} / {p95:6.1}   {hits:>4}    {b50:8.1} / {b95:6.1}");
    }
    println!("\nworst fast-path p95: {worst:.1} ms");
    if let Some(secs) = flag("--hold")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok())
    {
        println!(
            "holding pid {} for {secs}s for external memory measurement",
            std::process::id()
        );
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }
    if let Some(limit) = gate {
        if worst > limit {
            eprintln!("GATE FAILED: {worst:.1} ms > {limit} ms");
            std::process::exit(1);
        }
        println!("gate ok (<= {limit} ms)");
    }
}

fn rayon_threads() -> usize {
    std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        })
}
