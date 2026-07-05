# Cokret 加解密与授权支持任务列表

## 目标判断

octos 的 Cokret 接入已经具备 account/applet 两条基础消息路径、DID-proof 登录、事件签名、grant 校验和 applet transaction idempotency。为对齐最新 Cokret bridges / SDK 的 E2EE 能力，本轮补齐本地 crypto state、加解密策略、入站密文处理、出站 fail-closed、applet 设备身份和 HTTP Message Signature 授权校验。

## 并行关系

- A1、A2、A3 可以并行：分别确认依赖 feature、设计本地 crypto store、梳理 account/applet 入站密文 carrier。
- B1 依赖 A2/A3：需要明确 encrypted skip reason 后才能接入入站解密和 unable-to-decrypt 记录。
- B2 依赖 A2：出站加密需要 realm policy 与本地 group state。
- B3 依赖 A1：applet 的设备身份和 HTTP Message Signature 使用 SDK signer / runtime 能力。
- C1、C2、C3 在 B1-B3 之后可以并行：分别补 account 测试、applet 测试、编译和 clippy 验证。

## 已完成任务

- [x] A1 依赖与 feature 图：启用 Cokret `device-runtime` 和 `mls`，保留既有 `client`、`applet-runtime`、`full-surface`、`signer`。
- [x] A2 本地 crypto store：新增 `FileCokretCryptoStore`，按 account/applet scope 持久化 feature report、MLS backup、realm policy、bootstrap plan、key backup 状态和 unable-to-decrypt 记录。
- [x] A3 入站密文分类：account 与 applet 均识别 `encrypted_content`、`encrypted_payload`、`ck.content.encrypted`，避免把密文当普通 unsupported content 静默丢弃。
- [x] B1 入站解密：有本地 group state 时尝试解密并派发文本消息；缺 session 或坏密文时记录 unable-to-decrypt。
- [x] B2 出站加密：按 realm encryption policy 决定明文或密文发送；E2EE 必需但缺 group state 时 fail closed，阻止明文泄露。
- [x] B3 授权增强：applet 支持配置 `deviceId` 和 trusted verification methods，校验 source service DID、HTTP Message Signature、content digest，并将签名证据绑定到 idempotency anchor。
- [x] C1 account 覆盖：补充 encrypted carrier、realm policy 持久化、missing required group state fail-closed、bootstrap/key backup restore 相关测试。
- [x] C2 applet 覆盖：补充 trusted HTTP Message Signature 通过、篡改 body 拒绝、密文 carrier 识别与出站加密路径。
- [x] C3 targeted 验证：完成 `cargo fmt`、`cargo check`、`cargo test` 和 `cargo clippy` 的 Cokret 相关验证。

## 后续可扩展项

- [x] 记录 RRK/key-backup restore-needed 状态，等待 Cokret 服务端密钥备份事件或接口稳定后接入远端恢复。
- [x] 在 applet describe 中暴露 E2EE 能力、crypto store 路径和 HTTP Message Signature 要求，便于上游能力探测。
