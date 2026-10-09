//! Eksport zrzuconych slajdów do PDF i PPTX, bez zewnętrznych programów.

use image::codecs::jpeg::JpegEncoder;
use std::fs::File;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

type Res<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------------------------------------------------------- PDF

/// Jedna strona na slajd, strona ma proporcje obrazu. Obrazy osadzone jako JPEG.
pub fn to_pdf(slides: &[PathBuf], out: &Path) -> Res<()> {
    if slides.is_empty() {
        return Err("Brak slajdów do eksportu".into());
    }
    let mut buf: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();
    buf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    let n = slides.len();
    // numeracja obiektów: 1 katalog, 2 drzewo stron, potem po 3 na slajd (strona, treść, obraz)
    let page_id = |i: usize| 3 + i * 3;
    let mut objects: Vec<Vec<u8>> = Vec::new();

    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", page_id(i))).collect();
    objects
        .push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), n).into_bytes());

    for (i, path) in slides.iter().enumerate() {
        let img = image::open(path).map_err(err)?.to_rgb8();
        let (w, h) = img.dimensions();
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 92)
            .encode_image(&img)
            .map_err(err)?;

        let page_w = 960.0_f32;
        let page_h = page_w * h as f32 / w as f32;
        let (cid, iid) = (page_id(i) + 1, page_id(i) + 2);
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_w:.2} {page_h:.2}] \
                 /Resources << /XObject << /Im0 {iid} 0 R >> >> /Contents {cid} 0 R >>"
            )
            .into_bytes(),
        );
        let content = format!("q {page_w:.2} 0 0 {page_h:.2} 0 0 cm /Im0 Do Q");
        objects.push(
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                content.len(),
                content
            )
            .into_bytes(),
        );
        let mut obj = format!(
            "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpeg.len()
        )
        .into_bytes();
        obj.extend_from_slice(&jpeg);
        obj.extend_from_slice(b"\nendstream");
        objects.push(obj);
    }

    for (i, obj) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(obj);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    buf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    std::fs::write(out, buf).map_err(err)
}

// ---------------------------------------------------------------- PPTX

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const R_DOC: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Każdy slajd to jeden obraz wpasowany w slajd. Rozmiar slajdu bierzemy z proporcji pierwszego zrzutu.
pub fn to_pptx(slides: &[PathBuf], out: &Path) -> Res<()> {
    if slides.is_empty() {
        return Err("Brak slajdów do eksportu".into());
    }
    let dims: Vec<(u32, u32)> = slides
        .iter()
        .map(|p| image::image_dimensions(p).map_err(err))
        .collect::<Res<_>>()?;

    let slide_cx: i64 = 12_192_000; // 13.333 cala, standard 16:9
    let slide_cy: i64 = slide_cx * dims[0].1 as i64 / dims[0].0 as i64;

    let mut zip = zip::ZipWriter::new(File::create(out).map_err(err)?);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let put = |zip: &mut zip::ZipWriter<File>,
               name: &str,
               data: &[u8],
               o: SimpleFileOptions|
     -> Res<()> {
        zip.start_file(name, o).map_err(err)?;
        zip.write_all(data).map_err(err)
    };

    // [Content_Types].xml
    let mut ct = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Default Extension="png" ContentType="image/png"/>
<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>
<Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>
<Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
<Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>
"#,
    );
    for i in 1..=slides.len() {
        ct += &format!(
            r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>
"#
        );
    }
    ct += "</Types>";
    put(&mut zip, "[Content_Types].xml", ct.as_bytes(), opts)?;

    put(
        &mut zip,
        "_rels/.rels",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{REL_NS}">
<Relationship Id="rId1" Type="{R_DOC}/officeDocument" Target="ppt/presentation.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
<Relationship Id="rId3" Type="{R_DOC}/extended-properties" Target="docProps/app.xml"/>
</Relationships>"#
        )
        .as_bytes(),
        opts,
    )?;

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    put(
        &mut zip,
        "docProps/core.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<dc:title>Slajdy</dc:title><dc:creator>SlideCatcher</dc:creator>
<dcterms:created xsi:type="dcterms:W3CDTF">{now}</dcterms:created>
<dcterms:modified xsi:type="dcterms:W3CDTF">{now}</dcterms:modified>
</cp:coreProperties>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        &mut zip,
        "docProps/app.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>SlideCatcher</Application><Slides>{}</Slides></Properties>"#,
            slides.len()
        )
        .as_bytes(),
        opts,
    )?;

    // presentation.xml + rels
    let mut sld_ids = String::new();
    let mut pres_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{REL_NS}">
<Relationship Id="rId1" Type="{R_DOC}/slideMaster" Target="slideMasters/slideMaster1.xml"/>
<Relationship Id="rId2" Type="{R_DOC}/theme" Target="theme/theme1.xml"/>
"#
    );
    for i in 1..=slides.len() {
        sld_ids += &format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 255 + i, i + 2);
        pres_rels += &format!(
            r#"<Relationship Id="rId{}" Type="{R_DOC}/slide" Target="slides/slide{i}.xml"/>
"#,
            i + 2
        );
    }
    pres_rels += "</Relationships>";
    put(
        &mut zip,
        "ppt/presentation.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation {NS} saveSubsetFonts="1">
<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>
<p:sldIdLst>{sld_ids}</p:sldIdLst>
<p:sldSz cx="{slide_cx}" cy="{slide_cy}"/>
<p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        &mut zip,
        "ppt/_rels/presentation.xml.rels",
        pres_rels.as_bytes(),
        opts,
    )?;

    // master, layout, theme
    let empty_tree = r#"<p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>"#;
    put(
        &mut zip,
        "ppt/slideMasters/slideMaster1.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster {NS}>{}
<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>
</p:sldMaster>"#,
            empty_tree.replace("<p:cSld>", r#"<p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg>"#)
        )
        .as_bytes(),
        opts,
    )?;
    put(
        &mut zip,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{REL_NS}">
<Relationship Id="rId1" Type="{R_DOC}/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
<Relationship Id="rId2" Type="{R_DOC}/theme" Target="../theme/theme1.xml"/>
</Relationships>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(
        &mut zip,
        "ppt/slideLayouts/slideLayout1.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout {NS} type="blank" preserve="1">{}
<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sldLayout>"#,
            empty_tree.replace("<p:cSld>", r#"<p:cSld name="Pusty">"#)
        )
        .as_bytes(),
        opts,
    )?;
    put(
        &mut zip,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{REL_NS}">
<Relationship Id="rId1" Type="{R_DOC}/slideMaster" Target="../slideMasters/slideMaster1.xml"/>
</Relationships>"#
        )
        .as_bytes(),
        opts,
    )?;
    put(&mut zip, "ppt/theme/theme1.xml", THEME.as_bytes(), opts)?;

    // slajdy
    for (idx, (path, (w, h))) in slides.iter().zip(&dims).enumerate() {
        let i = idx + 1;
        // wpasuj obraz w slajd, zachowując proporcje (wyśrodkowany)
        let scale = (slide_cx as f64 / *w as f64).min(slide_cy as f64 / *h as f64);
        let cx = (*w as f64 * scale) as i64;
        let cy = (*h as f64 * scale) as i64;
        let x = (slide_cx - cx) / 2;
        let y = (slide_cy - cy) / 2;
        put(
            &mut zip,
            &format!("ppt/slides/slide{i}.xml"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld {NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
<p:pic><p:nvPicPr><p:cNvPr id="2" name="Slajd {i}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>
<p:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>
<p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>
</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
            )
            .as_bytes(),
            opts,
        )?;
        put(
            &mut zip,
            &format!("ppt/slides/_rels/slide{i}.xml.rels"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{REL_NS}">
<Relationship Id="rId1" Type="{R_DOC}/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
<Relationship Id="rId2" Type="{R_DOC}/image" Target="../media/image{i}.png"/>
</Relationships>"#
            )
            .as_bytes(),
            opts,
        )?;
        let png = png_bytes(path)?;
        put(&mut zip, &format!("ppt/media/image{i}.png"), &png, stored)?;
    }
    zip.finish().map_err(err)?;
    Ok(())
}

fn png_bytes(path: &Path) -> Res<Vec<u8>> {
    if path.extension().and_then(|e| e.to_str()) == Some("png") {
        return std::fs::read(path).map_err(err);
    }
    let img = image::open(path).map_err(err)?;
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .map_err(err)?;
    Ok(out.into_inner())
}

const THEME: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office">
<a:themeElements>
<a:clrScheme name="Office">
<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
<a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>
<a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2>
<a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4>
<a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6>
<a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink>
</a:clrScheme>
<a:fontScheme name="Office">
<a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>
<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>
</a:fontScheme>
<a:fmtScheme name="Office">
<a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst>
<a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>
<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>
<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst>
</a:fmtScheme>
</a:themeElements>
</a:theme>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn exports_pdf_and_pptx() {
        let dir = std::env::temp_dir().join("slidecatcher-test");
        std::fs::create_dir_all(&dir).unwrap();
        let mut paths = Vec::new();
        for (i, c) in [[200u8, 30, 30], [30, 160, 60], [30, 60, 200]]
            .iter()
            .enumerate()
        {
            let img = RgbaImage::from_pixel(1280, 720, Rgba([c[0], c[1], c[2], 255]));
            let p = dir.join(format!("s{i}.png"));
            img.save(&p).unwrap();
            paths.push(p);
        }
        to_pdf(&paths, &dir.join("out.pdf")).unwrap();
        to_pptx(&paths, &dir.join("out.pptx")).unwrap();
        assert!(std::fs::metadata(dir.join("out.pdf")).unwrap().len() > 1000);
        assert!(std::fs::metadata(dir.join("out.pptx")).unwrap().len() > 1000);
    }
}
