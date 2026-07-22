# RAG 2.0 移动同步

Windows 和 Android 使用同一份研究领域结构。Windows 负责采集、导入、向量生成和同步包导出；Android 导入后重建 FTS5，并以 BM25 离线查询。

## v2 同步包

同步包是真正的 SQLite 文件，`PRAGMA user_version=2`，格式标识为 `gp-research-pack-v2`。包只包含规范化文档及检索所需元数据：

- 不包含聊天会话和回答历史；
- 不包含向量 BLOB；
- 不包含用户未读状态；
- 限制为 64 MB、50,000 篇文档，单篇正文最多 4M 字符。

Android 导入后重建 FTS；Windows 导入后可在后台重新生成向量。导入使用同目录 staging 文件和原子重命名，成功替换前保留当前数据库为 `.rollback`。应用启动时会恢复中断的导入或回滚。

本机会话、回答、可匹配的引用和已读状态在同步包替换时保留。包中不存在的旧证据不会被伪造引用。

## 兼容迁移

读取顺序如下：

1. 校验文件大小和 16 字节 SQLite 头；
2. v2 SQLite 校验 `user_version` 和格式元数据；
3. 非 SQLite 文件按 v1 JSON 兼容解析；
4. 导入后统一写入 v2 数据库并重建 FTS。

旧包中的 `items`、`documents`、`source_documents`、`evidence_chunks` 和 `relation_edges` 会合并迁移；文档 ID 加入来源文件哈希和序号，避免不同旧表之间碰撞。

## Android 能力

Android 支持：

- 自选股消息、未读状态、事件流、历史问答；
- 本地 Jieba + FTS5 BM25 查询；
- 引用底部面板、原文 HTTPS 链接；
- v1/v2 包版本校验、原子替换和一键回滚。

Android 不显示建库和 embedding 管理，也不加载 Windows 的 ONNX 资源。网络不可用时仍可查询已经导入的本地资料。

## 安全边界

- URL/PDF 在线导入只在桌面知识库管理中提供；
- 同步包导入前执行大小、格式、文档数和正文总量限制；
- 包不携带可执行内容；
- 外部原文入口只允许 HTTPS；
- 社区来源在移动端同样不能单独支撑事实结论。
