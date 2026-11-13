//! data-iso: build a plain ISO 9660 + Joliet image from files, then read it
//! back like a player would (PVD/SVD, path tables, root directory records in
//! both the 8.3 and the Joliet UCS-2BE views, file extents) and verify that the
//! file contents are bit-exact.

use sacd_author::data_iso::{self, DataEntry};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

const SECTOR: usize = 2048;

fn sec(f: &mut File, lba: u32) -> Vec<u8> {
    f.seek(SeekFrom::Start(lba as u64 * SECTOR as u64)).unwrap();
    let mut buf = vec![0u8; SECTOR];
    f.read_exact(&mut buf).unwrap();
    buf
}

struct DirRec {
    extent: u32,
    size: u32,
    is_dir: bool,
    id: Vec<u8>,
}

fn parse_dir(data: &[u8]) -> Vec<DirRec> {
    let mut recs = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() && data[pos] != 0 {
        let len = data[pos] as usize;
        if len == 0 {
            break;
        }
        let rec = &data[pos..pos + len];
        let extent = u32::from_le_bytes(rec[2..6].try_into().unwrap());
        let size = u32::from_le_bytes(rec[10..14].try_into().unwrap());
        let is_dir = rec[25] & 0x02 != 0;
        let id_len = rec[32] as usize;
        let id = rec[33..33 + id_len].to_vec();
        recs.push(DirRec {
            extent,
            size,
            is_dir,
            id,
        });
        pos += len;
    }
    recs
}

fn ucs2be_to_string(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

#[test]
fn data_iso_roundtrip() {
    // three temp files, one with a long unicode name
    let files = [
        ("01 - Air.flac".to_string(), b"A".repeat(5000)),
        (
            "02 - La Telaraña de Dylan.flac".to_string(),
            b"B".repeat(3 * SECTOR + 17),
        ),
        ("notes.txt".to_string(), b"hello".to_vec()),
    ];
    let mut entries = Vec::new();
    let mut keep_alive = Vec::new();
    for (name, content) in &files {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        entries.push(DataEntry {
            name: name.clone(),
            path: f.path().to_path_buf(),
        });
        keep_alive.push(f);
    }

    let iso_file = tempfile::NamedTempFile::new().unwrap();
    let total = data_iso::build_data_iso(&entries, iso_file.path(), "TESTVOL").unwrap();
    assert_eq!(
        std::fs::metadata(iso_file.path()).unwrap().len(),
        total as u64 * SECTOR as u64
    );

    let mut iso = File::open(iso_file.path()).unwrap();

    // --- PVD ---
    let pvd = sec(&mut iso, 16);
    assert_eq!(pvd[0], 1);
    assert_eq!(&pvd[1..6], b"CD001");
    assert_eq!(&pvd[40..48], b"TESTVOL ");
    let vol_space = u32::from_le_bytes(pvd[80..84].try_into().unwrap());
    assert_eq!(vol_space, total);
    let pt_size = u32::from_le_bytes(pvd[132..136].try_into().unwrap());
    assert_eq!(pt_size, 10);
    assert_eq!(u32::from_le_bytes(pvd[140..144].try_into().unwrap()), 19);
    assert_eq!(u32::from_be_bytes(pvd[148..152].try_into().unwrap()), 20);
    let pvd_root_extent = u32::from_le_bytes(pvd[158..162].try_into().unwrap());
    assert_eq!(pvd_root_extent, 23);

    // --- SVD (Joliet) ---
    let svd = sec(&mut iso, 17);
    assert_eq!(svd[0], 2);
    assert_eq!(&svd[1..6], b"CD001");
    assert_eq!(&svd[88..91], b"%/E");
    assert_eq!(
        ucs2be_to_string(&svd[40..72]).trim_end_matches('\0'),
        "TESTVOL"
    );
    let svd_root_extent = u32::from_le_bytes(svd[158..162].try_into().unwrap());
    assert!(svd_root_extent >= 24);
    assert_eq!(u32::from_le_bytes(svd[140..144].try_into().unwrap()), 21);
    assert_eq!(u32::from_be_bytes(svd[148..152].try_into().unwrap()), 22);

    // --- terminator + path tables ---
    let term = sec(&mut iso, 18);
    assert_eq!(term[0], 255);
    assert_eq!(&term[1..6], b"CD001");
    let ptl = sec(&mut iso, 19);
    assert_eq!(ptl[0], 1); // len_di of root
    assert_eq!(ptl[1], 0);
    assert_eq!(u32::from_le_bytes(ptl[2..6].try_into().unwrap()), 23);
    let ptm = sec(&mut iso, 20);
    assert_eq!(u32::from_be_bytes(ptm[2..6].try_into().unwrap()), 23);

    // --- root directory, ISO 9660 view ---
    let root_pvd = sec(&mut iso, 23);
    let recs = parse_dir(&root_pvd);
    assert_eq!(recs[0].id, vec![0]);
    assert_eq!(recs[1].id, vec![1]);
    let files_pvd: Vec<&DirRec> = recs.iter().skip(2).filter(|r| !r.is_dir).collect();
    assert_eq!(files_pvd.len(), 3);
    for r in &files_pvd {
        let id = String::from_utf8_lossy(&r.id).to_string();
        assert!(id.ends_with(";1"), "mangled id: {id}");
        let stem = id.strip_suffix(";1").unwrap();
        let (base, ext) = stem.split_once('.').unwrap_or((stem, ""));
        assert!(base.len() <= 8 && ext.len() <= 3, "not 8.3: {id}");
    }

    // --- root directory, Joliet view: real names ---
    let root_svd = sec(&mut iso, svd_root_extent);
    let recs_j = parse_dir(&root_svd);
    let mut names_j: Vec<String> = recs_j
        .iter()
        .skip(2)
        .filter(|r| !r.is_dir)
        .map(|r| ucs2be_to_string(&r.id))
        .collect();
    names_j.sort();
    let mut expected: Vec<String> = files.iter().map(|(n, _)| n.clone()).collect();
    expected.sort();
    assert_eq!(names_j, expected);

    // --- file contents are bit-exact ---
    for (r, (_, content)) in recs_j
        .iter()
        .skip(2)
        .filter(|r| !r.is_dir)
        .zip(files.iter())
    {
        assert_eq!(r.size as usize, content.len());
        iso.seek(SeekFrom::Start(r.extent as u64 * SECTOR as u64))
            .unwrap();
        let mut got = vec![0u8; r.size as usize];
        iso.read_exact(&mut got).unwrap();
        assert_eq!(&got, content);
    }
}
