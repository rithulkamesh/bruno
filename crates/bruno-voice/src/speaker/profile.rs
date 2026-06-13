use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct VoiceProfile {
    pub embedding: Vec<f32>,
}

impl VoiceProfile {
    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        if bytes.len() < 8 {
            return Err("corrupt profile".into());
        }
        let dim = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        let expected = 4 + dim * 4;
        if bytes.len() != expected {
            return Err("corrupt profile size".into());
        }
        let mut embedding = Vec::with_capacity(dim);
        for i in 0..dim {
            let off = 4 + i * 4;
            embedding.push(f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()));
        }
        Ok(Some(Self { embedding }))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let dim = self.embedding.len() as u32;
        let mut bytes = Vec::with_capacity(4 + self.embedding.len() * 4);
        bytes.extend_from_slice(&dim.to_le_bytes());
        for v in &self.embedding {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        fs::write(path, bytes).map_err(|e| e.to_string())
    }
}
