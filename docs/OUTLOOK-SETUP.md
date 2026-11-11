# Outlook 微软应用配置（维护者）

Outlook 接入使用项目自己的 Microsoft Entra 应用注册。项目目前没有可用的注册，未配置客户端 ID 的构建会提示维护者先完成配置；它不能连接真实 Outlook 邮箱。不要借用其他应用的客户端 ID，也不要提交客户端密钥、用户密码或令牌。

1. 在具备应用注册权限的 Microsoft Entra 租户中创建项目应用。微软的[应用注册说明](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app)列出了账户、订阅和权限前提。
2. 将支持的账户类型设置为“任何组织目录中的账户和个人 Microsoft 账户”。添加“移动和桌面应用程序”平台，注册回调 URI `http://localhost/gua-outlook`。桌面应用使用随机本机端口，微软对 localhost 回调匹配允许端口变化。不要创建或内置客户端密钥。
3. 应用使用委托授权，不使用应用程序权限。授权请求的权限为 `https://outlook.office.com/IMAP.AccessAsUser.All` 和 `offline_access`。请核实租户对 Exchange Online 资源的委托权限和用户同意策略；如果组织要求管理员同意，请由相应管理员批准。IMAP 权限允许邮箱读写，呱只进行读取。详情见[微软 IMAP OAuth 文档](https://learn.microsoft.com/en-us/exchange/client-developer/legacy-protocols/how-to-authenticate-an-imap-pop-smtp-application-by-using-oauth)。
4. 从应用概览复制公开的“应用程序（客户端）ID”，在构建和打包进程中设置环境变量，再构建：

   ```powershell
   $env:GUA_OUTLOOK_CLIENT_ID = '00000000-0000-0000-0000-000000000000' # 替换为项目应用 ID
   cargo build --locked --release --bin gua
   # 如果使用 scripts/package.ps1，应在同一个环境中运行
   ```

   此 ID 在编译时嵌入程序，并非机密。不同的 ID 需要重新构建；不要把上面的示例 ID 当作有效注册。
5. 使用经过授权的个人 Outlook 和 Microsoft 365 工作邮箱分别测试登录、收取新邮件、验证码复制、重启续期和删除。个人邮箱可能需要先开启 IMAP；组织策略或未验证的发布者可能要求管理员批准。只有完成真实账号测试后，才能将相应账号类型标为已实测。
