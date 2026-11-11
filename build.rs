use std::env;
use std::fs;
use std::path::PathBuf;

/// Construit une structure binaire `VS_VERSIONINFO` valide (UTF-16LE alignée sur 4 octets)
/// pour que l'Explorateur Windows et les EDR (SentinelOne, Defender) identifient l'exécutable.
fn build_vs_version_info() -> Vec<u8> {
    fn push_u16(buf: &mut Vec<u8>, v: u16) {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    fn push_u32(buf: &mut Vec<u8>, v: u32) {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    fn push_wstr(buf: &mut Vec<u8>, s: &str) {
        for ch in s.encode_utf16() {
            push_u16(buf, ch);
        }
        push_u16(buf, 0);
    }
    fn align4(buf: &mut Vec<u8>) {
        while buf.len() % 4 != 0 {
            buf.push(0);
        }
    }

    fn make_string_entry(key: &str, value: &str) -> Vec<u8> {
        let mut b = Vec::new();
        push_u16(&mut b, 0); // wLength placeholder
        let val_words = (value.encode_utf16().count() + 1) as u16;
        push_u16(&mut b, val_words); // wValueLength (en mots u16)
        push_u16(&mut b, 1); // wType = 1 (texte)
        push_wstr(&mut b, key);
        align4(&mut b);
        push_wstr(&mut b, value);
        let len = b.len() as u16;
        b[0..2].copy_from_slice(&len.to_le_bytes());
        align4(&mut b);
        b
    }

    let pairs = [
        ("CompanyName", "Magic Mouse Studio"),
        (
            "FileDescription",
            "Magic Mouse Studio - Utilitaire Multi-Touch & Inertie pour Windows",
        ),
        ("FileVersion", "0.1.0.0"),
        ("InternalName", "magic-mouse-win.exe"),
        ("LegalCopyright", "Copyright (c) 2026 Magic Mouse Studio"),
        ("OriginalFilename", "magic-mouse-win.exe"),
        ("ProductName", "Magic Mouse Studio"),
        ("ProductVersion", "0.1.0.0"),
    ];

    // StringTable "040904B0" (US English / Unicode)
    let mut string_table = Vec::new();
    push_u16(&mut string_table, 0);
    push_u16(&mut string_table, 0);
    push_u16(&mut string_table, 1);
    push_wstr(&mut string_table, "040904B0");
    align4(&mut string_table);
    for (k, v) in pairs {
        string_table.extend_from_slice(&make_string_entry(k, v));
    }
    let st_len = string_table.len() as u16;
    string_table[0..2].copy_from_slice(&st_len.to_le_bytes());
    align4(&mut string_table);

    // StringFileInfo
    let mut sfi = Vec::new();
    push_u16(&mut sfi, 0);
    push_u16(&mut sfi, 0);
    push_u16(&mut sfi, 1);
    push_wstr(&mut sfi, "StringFileInfo");
    align4(&mut sfi);
    sfi.extend_from_slice(&string_table);
    let sfi_len = sfi.len() as u16;
    sfi[0..2].copy_from_slice(&sfi_len.to_le_bytes());
    align4(&mut sfi);

    // Var ("Translation")
    let mut var_entry = Vec::new();
    push_u16(&mut var_entry, 0);
    push_u16(&mut var_entry, 4); // 4 octets de valeur
    push_u16(&mut var_entry, 0); // wType = 0 (binaire)
    push_wstr(&mut var_entry, "Translation");
    align4(&mut var_entry);
    push_u16(&mut var_entry, 0x0409);
    push_u16(&mut var_entry, 0x04B0);
    let var_len = var_entry.len() as u16;
    var_entry[0..2].copy_from_slice(&var_len.to_le_bytes());
    align4(&mut var_entry);

    // VarFileInfo
    let mut vfi = Vec::new();
    push_u16(&mut vfi, 0);
    push_u16(&mut vfi, 0);
    push_u16(&mut vfi, 1);
    push_wstr(&mut vfi, "VarFileInfo");
    align4(&mut vfi);
    vfi.extend_from_slice(&var_entry);
    let vfi_len = vfi.len() as u16;
    vfi[0..2].copy_from_slice(&vfi_len.to_le_bytes());
    align4(&mut vfi);

    // Racine VS_VERSION_INFO
    let mut root = Vec::new();
    push_u16(&mut root, 0); // wLength placeholder
    push_u16(&mut root, 52); // sizeof(VS_FIXEDFILEINFO) = 52
    push_u16(&mut root, 0); // wType = 0
    push_wstr(&mut root, "VS_VERSION_INFO");
    align4(&mut root);

    // VS_FIXEDFILEINFO (52 octets)
    push_u32(&mut root, 0xFEEF04BD); // dwSignature
    push_u32(&mut root, 0x00010000); // dwStrucVersion
    push_u32(&mut root, 0x00000001); // dwFileVersionMS (0.1)
    push_u32(&mut root, 0x00000000); // dwFileVersionLS (0.0)
    push_u32(&mut root, 0x00000001); // dwProductVersionMS (0.1)
    push_u32(&mut root, 0x00000000); // dwProductVersionLS (0.0)
    push_u32(&mut root, 0x0000003F); // dwFileFlagsMask
    push_u32(&mut root, 0x00000000); // dwFileFlags
    push_u32(&mut root, 0x00040004); // dwFileOS = VOS_NT_WINDOWS32
    push_u32(&mut root, 0x00000001); // dwFileType = VFT_APP
    push_u32(&mut root, 0x00000000); // dwFileSubtype
    push_u32(&mut root, 0x00000000); // dwFileDateMS
    push_u32(&mut root, 0x00000000); // dwFileDateLS
    align4(&mut root);

    root.extend_from_slice(&sfi);
    root.extend_from_slice(&vfi);

    let total_len = root.len() as u16;
    root[0..2].copy_from_slice(&total_len.to_le_bytes());
    root
}

/// Parse `assets/icon.ico` et retourne `(RT_ICON entries, RT_GROUP_ICON blob)` pour les ressources PE.
fn parse_ico_resources(ico_bytes: &[u8]) -> Option<(Vec<(u32, Vec<u8>)>, Vec<u8>)> {
    if ico_bytes.len() < 6 {
        return None;
    }
    let reserved = u16::from_le_bytes([ico_bytes[0], ico_bytes[1]]);
    let ico_type = u16::from_le_bytes([ico_bytes[2], ico_bytes[3]]);
    let count = u16::from_le_bytes([ico_bytes[4], ico_bytes[5]]) as usize;
    if reserved != 0 || ico_type != 1 || count == 0 || ico_bytes.len() < 6 + count * 16 {
        return None;
    }

    let mut icons = Vec::with_capacity(count);
    let mut grp = Vec::with_capacity(6 + count * 14);
    grp.extend_from_slice(&ico_bytes[0..6]);

    for i in 0..count {
        let off = 6 + i * 16;
        let entry = &ico_bytes[off..off + 16];
        let bytes_in_res =
            u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
        let image_offset =
            u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
        let end = image_offset.checked_add(bytes_in_res)?;
        if end > ico_bytes.len() {
            return None;
        }
        let icon_id = (i + 1) as u32;
        icons.push((icon_id, ico_bytes[image_offset..end].to_vec()));
        grp.extend_from_slice(&entry[0..12]);
        grp.extend_from_slice(&(icon_id as u16).to_le_bytes());
    }

    Some((icons, grp))
}

/// Génère un fichier objet COFF 64 bits (`.o`) contenant l'icône (`RT_ICON` 3 + `RT_GROUP_ICON` 14),
/// la ressource `RT_VERSION` (16) et le manifeste UAC `asInvoker` `RT_MANIFEST` (24) sans `windres.exe`.
fn write_pe_resource_coff(path: &PathBuf) {
    let version_blob = build_vs_version_info();
    let manifest_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="0.1.0.0" processorArchitecture="amd64" name="MagicMouseStudio.MagicMouseWin" type="win32"/>
  <description>Magic Mouse Studio - Utilitaire Multi-Touch Apple pour Windows</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
    </application>
  </compatibility>
</assembly>"#;

    // Collecte des ressources (type_id, name_id, lang_id, data)
    let mut raw_entries: Vec<(u32, u32, u32, Vec<u8>)> = Vec::new();
    if let Ok(ico_bytes) = fs::read("assets/icon.ico") {
        if let Some((icons, grp)) = parse_ico_resources(&ico_bytes) {
            for (icon_id, data) in icons {
                raw_entries.push((3, icon_id, 1033, data)); // RT_ICON = 3
            }
            raw_entries.push((14, 1, 1033, grp)); // RT_GROUP_ICON = 14
        }
    }
    raw_entries.push((16, 1, 1033, version_blob)); // RT_VERSION = 16
    raw_entries.push((24, 1, 1033, manifest_xml.to_vec())); // RT_MANIFEST = 24
    raw_entries.sort_by_key(|(t, n, l, _)| (*t, *n, *l));

    // Regroupement hiérarchique Type -> Name -> Lang
    let mut tree: Vec<(u32, Vec<(u32, Vec<(u32, Vec<u8>)>)>)> = Vec::new();
    for (t, n, l, data) in raw_entries {
        if tree.last().map(|x| x.0) != Some(t) {
            tree.push((t, Vec::new()));
        }
        let names = &mut tree.last_mut().unwrap().1;
        if names.last().map(|x| x.0) != Some(n) {
            names.push((n, Vec::new()));
        }
        names.last_mut().unwrap().1.push((l, data));
    }

    // Calcul des offsets dans .rsrc$01
    let root_dir_size = 16 + 8 * tree.len();
    let mut type_dirs_size = 0usize;
    let mut lang_dirs_size = 0usize;
    let mut leaf_count = 0usize;
    for (_, names) in &tree {
        type_dirs_size += 16 + 8 * names.len();
        for (_, langs) in names {
            lang_dirs_size += 16 + 8 * langs.len();
            leaf_count += langs.len();
        }
    }
    let _total_rsrc01_size = root_dir_size + type_dirs_size + lang_dirs_size + leaf_count * 16;

    let push_dir = |b: &mut Vec<u8>, id_count: usize| {
        b.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
        b.extend_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
        b.extend_from_slice(&0u16.to_le_bytes()); // MajorVersion
        b.extend_from_slice(&0u16.to_le_bytes()); // MinorVersion
        b.extend_from_slice(&0u16.to_le_bytes()); // NumberOfNamedEntries
        b.extend_from_slice(&(id_count as u16).to_le_bytes()); // NumberOfIdEntries
    };
    let push_entry = |b: &mut Vec<u8>, id: u32, offset: u32, is_dir: bool| {
        b.extend_from_slice(&id.to_le_bytes());
        let off = if is_dir { offset | 0x8000_0000 } else { offset };
        b.extend_from_slice(&off.to_le_bytes());
    };

    let mut rsrc01 = Vec::with_capacity(_total_rsrc01_size);
    let mut rsrc02 = Vec::new();
    let mut reloc_rvas: Vec<u32> = Vec::with_capacity(leaf_count);

    // 1. Root Directory
    push_dir(&mut rsrc01, tree.len());
    let mut cur_type_off = root_dir_size as u32;
    for (type_id, names) in &tree {
        push_entry(&mut rsrc01, *type_id, cur_type_off, true);
        cur_type_off += (16 + 8 * names.len()) as u32;
    }

    // 2. Type Subdirectories
    let mut cur_lang_off = (root_dir_size + type_dirs_size) as u32;
    for (_, names) in &tree {
        push_dir(&mut rsrc01, names.len());
        for (name_id, langs) in names {
            push_entry(&mut rsrc01, *name_id, cur_lang_off, true);
            cur_lang_off += (16 + 8 * langs.len()) as u32;
        }
    }

    // 3. Lang Subdirectories
    let mut cur_data_entry_off = (root_dir_size + type_dirs_size + lang_dirs_size) as u32;
    for (_, names) in &tree {
        for (_, langs) in names {
            push_dir(&mut rsrc01, langs.len());
            for (lang_id, _) in langs {
                push_entry(&mut rsrc01, *lang_id, cur_data_entry_off, false);
                cur_data_entry_off += 16;
            }
        }
    }

    // 4. IMAGE_RESOURCE_DATA_ENTRY leaves + .rsrc$02 payloads
    for (_, names) in &tree {
        for (_, langs) in names {
            for (_, data) in langs {
                let data_offset_in_02 = rsrc02.len() as u32;
                let data_size = data.len() as u32;
                rsrc02.extend_from_slice(data);
                while rsrc02.len() % 8 != 0 {
                    rsrc02.push(0);
                }

                let entry_rva = rsrc01.len() as u32;
                reloc_rvas.push(entry_rva);
                rsrc01.extend_from_slice(&data_offset_in_02.to_le_bytes()); // OffsetToData (relocated)
                rsrc01.extend_from_slice(&data_size.to_le_bytes());
                rsrc01.extend_from_slice(&0u32.to_le_bytes()); // CodePage
                rsrc01.extend_from_slice(&0u32.to_le_bytes()); // Reserved
            }
        }
    }

    // Assemblage du fichier COFF x86_64
    // Header COFF (20 octets) + 2 Section Headers (2 * 40 = 80 octets) = 100 octets
    let sec1_raw_ptr = 100u32;
    let sec1_size = rsrc01.len() as u32;
    let sec1_reloc_ptr = sec1_raw_ptr + sec1_size;
    let sec1_reloc_count = reloc_rvas.len() as u16;
    let sec2_raw_ptr = sec1_reloc_ptr + (reloc_rvas.len() as u32) * 10;
    let sec2_size = rsrc02.len() as u32;
    let symtab_ptr = sec2_raw_ptr + sec2_size;
    let num_symbols = 1u32; // Symbole 0 : ".rsrc$02" (Section 2)

    let mut coff = Vec::new();
    // 1. IMAGE_FILE_HEADER (20 octets)
    coff.extend_from_slice(&0x8664u16.to_le_bytes()); // Machine = AMD64
    coff.extend_from_slice(&2u16.to_le_bytes()); // NumberOfSections = 2
    coff.extend_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
    coff.extend_from_slice(&symtab_ptr.to_le_bytes()); // PointerToSymbolTable
    coff.extend_from_slice(&num_symbols.to_le_bytes()); // NumberOfSymbols = 1
    coff.extend_from_slice(&0u16.to_le_bytes()); // SizeOfOptionalHeader = 0
    coff.extend_from_slice(&0u16.to_le_bytes()); // Characteristics

    let flags = 0x4000_0040u32; // IMAGE_SCN_CNT_INITIALIZED_DATA | IMAGE_SCN_MEM_READ

    // 2. Section Header 1 : ".rsrc$01" (40 octets)
    coff.extend_from_slice(b".rsrc$01");
    coff.extend_from_slice(&0u32.to_le_bytes()); // VirtualSize
    coff.extend_from_slice(&0u32.to_le_bytes()); // VirtualAddress
    coff.extend_from_slice(&sec1_size.to_le_bytes()); // SizeOfRawData
    coff.extend_from_slice(&sec1_raw_ptr.to_le_bytes()); // PointerToRawData
    coff.extend_from_slice(&sec1_reloc_ptr.to_le_bytes()); // PointerToRelocations
    coff.extend_from_slice(&0u32.to_le_bytes()); // PointerToLinenumbers
    coff.extend_from_slice(&sec1_reloc_count.to_le_bytes()); // NumberOfRelocations
    coff.extend_from_slice(&0u16.to_le_bytes()); // NumberOfLinenumbers
    coff.extend_from_slice(&flags.to_le_bytes()); // Characteristics

    // 3. Section Header 2 : ".rsrc$02" (40 octets)
    coff.extend_from_slice(b".rsrc$02");
    coff.extend_from_slice(&0u32.to_le_bytes()); // VirtualSize
    coff.extend_from_slice(&0u32.to_le_bytes()); // VirtualAddress
    coff.extend_from_slice(&sec2_size.to_le_bytes()); // SizeOfRawData
    coff.extend_from_slice(&sec2_raw_ptr.to_le_bytes()); // PointerToRawData
    coff.extend_from_slice(&0u32.to_le_bytes()); // PointerToRelocations
    coff.extend_from_slice(&0u32.to_le_bytes()); // PointerToLinenumbers
    coff.extend_from_slice(&0u16.to_le_bytes()); // NumberOfRelocations
    coff.extend_from_slice(&0u16.to_le_bytes()); // NumberOfLinenumbers
    coff.extend_from_slice(&flags.to_le_bytes()); // Characteristics

    // 4. Données de .rsrc$01
    coff.extend_from_slice(&rsrc01);

    // 5. Relocations pour .rsrc$01 (IMAGE_REL_AMD64_ADDR32NB = 0x0003 vers symbole 0)
    for rva_off in reloc_rvas {
        coff.extend_from_slice(&rva_off.to_le_bytes()); // VirtualAddress
        coff.extend_from_slice(&0u32.to_le_bytes()); // SymbolTableIndex = 0
        coff.extend_from_slice(&0x0003u16.to_le_bytes()); // Type = IMAGE_REL_AMD64_ADDR32NB
    }

    // 6. Données de .rsrc$02
    coff.extend_from_slice(&rsrc02);

    // 7. Table des symboles (1 symbole de 18 octets : ".rsrc$02" dans Section 2)
    coff.extend_from_slice(b".rsrc$02");
    coff.extend_from_slice(&0u32.to_le_bytes()); // Value = 0
    coff.extend_from_slice(&2i16.to_le_bytes()); // SectionNumber = 2
    coff.extend_from_slice(&0u16.to_le_bytes()); // Type = 0
    coff.push(3); // StorageClass = IMAGE_SYM_CLASS_STATIC (3)
    coff.push(0); // NumberOfAuxSymbols = 0

    // 8. Table de chaînes vide (4 octets = taille 4)
    coff.extend_from_slice(&4u32.to_le_bytes());

    let _ = fs::write(path, coff);
}

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:rustc-link-search=native={}", out_dir.display());

    // Génère et lie les ressources PE Windows (VERSIONINFO + Manifeste UAC asInvoker)
    let res_obj = out_dir.join("magic_mouse_win_res.o");
    write_pe_resource_coff(&res_obj);
    println!("cargo:rustc-link-arg-bins={}", res_obj.display());

    // Si `libwinapi_shlwapi.a` est disponible dans le cache Cargo, on le copie sous le nom `libshlwapi.a`
    // pour satisfaire l'éditeur de liens autonome (`self-contained` GNU) sans MSYS2 externe.
    if let Ok(home) = env::var("USERPROFILE") {
        let registry_src = PathBuf::from(home).join(".cargo\\registry\\src");
        if let Ok(entries) = fs::read_dir(registry_src) {
            for entry in entries.flatten() {
                let winapi_lib = entry
                    .path()
                    .join("winapi-x86_64-pc-windows-gnu-0.4.0\\lib\\libwinapi_shlwapi.a");
                if winapi_lib.exists() {
                    let _ = fs::copy(&winapi_lib, out_dir.join("libshlwapi.a"));
                    break;
                }
            }
        }
    }
}
