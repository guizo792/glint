use std::time::Instant;
use windows::core::{Interface, Result};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::DeleteObject;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItemImageFactory,
    SHGetKnownFolderItem, KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
    SIIGBF_ICONONLY,
};

fn main() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let t = Instant::now();
        let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
        let en: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
        let mut apps: Vec<(String, String, IShellItem)> = Vec::new();
        loop {
            let mut buf: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0u32;
            let _ = en.Next(&mut buf, Some(&mut fetched));
            let Some(item) = buf[0].take() else { break };
            let name = item.GetDisplayName(SIGDN_NORMALDISPLAY)?;
            let id = item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING)?;
            apps.push((name.to_string()?, id.to_string()?, item));
            CoTaskMemFree(Some(name.0 as _));
            CoTaskMemFree(Some(id.0 as _));
        }
        println!("enumerated {} apps in {:?}", apps.len(), t.elapsed());
        let uwp = apps.iter().filter(|a| a.1.contains('!')).count();
        println!("  UWP/packaged (id has '!'): {uwp}; classic: {}", apps.len() - uwp);
        for a in apps.iter().take(6) {
            println!("  {:<32} {}", a.0, a.1);
        }

        let t = Instant::now();
        let (mut ok, mut times) = (0usize, Vec::new());
        for (_, _, item) in &apps {
            let s = Instant::now();
            let f: IShellItemImageFactory = item.cast()?;
            if let Ok(bmp) = f.GetImage(SIZE { cx: 32, cy: 32 }, SIIGBF_ICONONLY) {
                ok += 1;
                let _ = DeleteObject(bmp.into());
            }
            times.push(s.elapsed().as_secs_f64() * 1e3);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "icons: {ok}/{} fetched in {:?}; per-icon p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
            apps.len(), t.elapsed(), times[times.len() / 2], times[times.len() * 95 / 100], times[times.len() - 1]
        );
    }
    Ok(())
}
