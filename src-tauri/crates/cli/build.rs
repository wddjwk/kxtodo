//! Windows MSVC：嵌入进程级 UTF-8 代码页 manifest（GBK 控制台下中文参数必乱码）。

fn main() {
    #[cfg(target_env = "msvc")]
    {
        use embed_manifest::manifest::ActiveCodePage::Utf8;
        use embed_manifest::{embed_manifest, new_manifest};
        let manifest = new_manifest("kxtodo-cli").active_code_page(Utf8);
        if let Err(error) = embed_manifest(manifest) {
            panic!("嵌入 manifest 失败：{error}");
        }
    }
}
