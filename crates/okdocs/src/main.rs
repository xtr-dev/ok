//! `okdocs build DOCS SITE`: write the guide in DOCS (the repository's
//! `docs/`) into SITE/docs/, and add its pages to SITE/sitemap.xml. The
//! Pages workflow runs it after copying `site/` into SITE.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [cmd, docs, site] = args.as_slice() else {
        eprintln!("usage: okdocs build DOCS SITE");
        return ExitCode::from(2);
    };
    if cmd != "build" {
        eprintln!("usage: okdocs build DOCS SITE");
        return ExitCode::from(2);
    }
    match build(Path::new(docs), Path::new(site)) {
        Ok(n) => {
            println!("wrote {n} pages to {site}/docs/");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("okdocs: {e}");
            ExitCode::FAILURE
        }
    }
}

fn build(docs: &Path, site: &Path) -> Result<usize, String> {
    let (sections, pages) = okdocs::load(docs)?;
    for (i, page) in pages.iter().enumerate() {
        let dir = site.join(page.path().trim_start_matches('/'));
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let html = okdocs::render_page(&sections, &pages, i);
        std::fs::write(dir.join("index.html"), html).map_err(|e| e.to_string())?;
    }
    // The sitemap in site/ lists the home page; add every guide page.
    let sitemap = site.join("sitemap.xml");
    let text =
        std::fs::read_to_string(&sitemap).map_err(|e| format!("{}: {e}", sitemap.display()))?;
    let urls: String = pages
        .iter()
        .map(|p| format!("  <url><loc>{}{}</loc></url>\n", okdocs::SITE, p.path()))
        .collect();
    let text = text.replacen("</urlset>", &format!("{urls}</urlset>"), 1);
    std::fs::write(&sitemap, text).map_err(|e| e.to_string())?;
    Ok(pages.len())
}
