//! 显式授予的本地常规文件探针；异步读在阻塞池，后台 VM 只等待有限结果。
use super::*;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;

pub struct FileProbe {
    file: Arc<Mutex<std::fs::File>>,
    max_bytes: usize,
}
impl FileProbe {
    pub fn open(path: &Path, max_bytes: usize) -> ModHostResult<Self> {
        if !(1..=8 * 1024 * 1024).contains(&max_bytes) {
            return Err(error("文件探针字节限额非法"));
        }
        let metadata = std::fs::metadata(path).map_err(|e| error(e.to_string()))?;
        if !metadata.is_file() {
            return Err(error("文件探针只接受常规文件"));
        }
        let mut open = std::fs::OpenOptions::new();
        open.read(true);
        // 元数据预检与打开间可能被换成 FIFO；Unix 采用非阻塞打开，
        // 随后按已打开描述符复验常规文件，避免后台启动卡在无写端管道。
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open.custom_flags(libc::O_NONBLOCK);
        }
        let file = open.open(path).map_err(|e| error(e.to_string()))?;
        if !file.metadata().map_err(|e| error(e.to_string()))?.is_file() {
            return Err(error("文件探针只接受常规文件"));
        }
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
            max_bytes,
        })
    }
}
impl ProbeReader for FileProbe {
    fn read(&self) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + '_>> {
        let file = self.file.clone();
        let max_bytes = self.max_bytes;
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut file = file.lock().map_err(|e| e.to_string())?;
                file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
                let mut bytes = Vec::new();
                (&mut *file)
                    .take(max_bytes as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > max_bytes {
                    return Err("文件探针输出超限".into());
                }
                let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                Ok(Value::String(text))
            })
            .await
            .map_err(|e| e.to_string())?
        })
    }
}
