//! SSH 连接配置：从 config.json 读取
use serde::Deserialize;

/// SSH 连接配置（从 demo 目录下的 `config.json` 读取）
#[derive(Debug, Clone, Deserialize)]
pub struct SshConfig {
    /// SSH 服务器主机名/IP
    pub host: String,
    /// SSH 服务器端口（默认 22）
    #[serde(default = "default_port")]
    pub port: u16,
    /// 登录用户名
    pub username: String,
    /// 登录密码（密码认证）
    pub password: String,
}

fn default_port() -> u16 {
    22
}

impl SshConfig {
    /// 从指定路径加载配置
    ///
    /// 路径可以是绝对路径或相对路径（相对于进程 CWD）。
    /// 文件格式：JSON，字段见 [`SshConfig`]。
    pub fn load_from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("读取配置文件 {path} 失败: {e}"))?;
        let cfg: Self = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("解析配置文件 {path} 失败: {e}"))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// 校验配置字段非空
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.host.trim().is_empty() {
            anyhow::bail!("config.host 不能为空");
        }
        if self.username.trim().is_empty() {
            anyhow::bail!("config.username 不能为空");
        }
        if self.password.is_empty() {
            // 允许空密码（部分测试服务器），但记录警告
            eprintln!("[russh-slint-demo] 警告: config.password 为空");
        }
        if self.port == 0 {
            anyhow::bail!("config.port 不能为 0");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_config() {
        let json = r#"{
            "host": "192.168.1.10",
            "port": 2222,
            "username": "alice",
            "password": "secret"
        }"#;
        let cfg: SshConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.host, "192.168.1.10");
        assert_eq!(cfg.port, 2222);
        assert_eq!(cfg.username, "alice");
        assert_eq!(cfg.password, "secret");
    }

    #[test]
    fn test_default_port_when_missing() {
        let json = r#"{
            "host": "example.com",
            "username": "bob",
            "password": "pw"
        }"#;
        let cfg: SshConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.port, 22, "缺省 port 应为 22");
    }

    #[test]
    fn test_validate_rejects_empty_host() {
        let cfg = SshConfig {
            host: "  ".to_string(),
            port: 22,
            username: "u".into(),
            password: "p".into(),
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_zero_port() {
        let cfg = SshConfig {
            host: "h".to_string(),
            port: 0,
            username: "u".into(),
            password: "p".into(),
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_load_from_file_missing() {
        let res = SshConfig::load_from_file("/nonexistent/path/config.json");
        assert!(res.is_err(), "不存在路径应返回错误");
    }

    #[test]
    fn test_load_from_file_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"host":"127.0.0.1","port":22,"username":"u","password":"p"}"#,
        )
        .unwrap();
        let cfg = SshConfig::load_from_file(path.to_str().unwrap()).unwrap();
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.username, "u");
    }

    #[test]
    fn test_load_from_file_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "not json").unwrap();
        let res = SshConfig::load_from_file(path.to_str().unwrap());
        assert!(res.is_err());
    }
}
