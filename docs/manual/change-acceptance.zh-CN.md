---
layout: docs
title: Change Acceptance
description: 用真实执行与当前 revision 证明改动是否可进入下一阶段
lang: zh-CN
alternate: /docs/change-acceptance/
permalink: /zh/docs/change-acceptance/
---

# Change Acceptance

继续使用已有 Coding Agent。wcode 从当前 Git、已批准的项目 Policy、持久化 Verification 和 Evidence 捕获 Change Acceptance Record。新增实现正在工作树中集成验证；不代表已发布或已部署 GitHub gate。

## 本地闭环

先通过 [Policy 操作者流程](../acceptance-policy/) 批准本地项目基线。然后在 MCP 中调用：

```json
{"action":"inspect"}
```

工具名是 `change_acceptance`。默认观察 HEAD 到 worktree；明确 PR 候选时同时提供完整 `base_revision` 和 `target_revision` SHA。不能输入 Evidence、PASS、actor 或 approval JSON。

- `inspect`：捕获当前 Record，不运行验证，也不追加历史。
- `plan`：冻结指定候选的 Policy 要求、风险底线和原生命令绑定。
- `verify`：执行该候选所需的完整原生矩阵与 Stage；缺失 executor 或人工要求继续阻止验收。
- `record`：追加有界本地历史并重新确认输入。
- `history`：读取历史；历史记录不能证明当前 revision。

CLI 也提供相应子命令：

```text
wcode acceptance inspect --base <完整SHA> --head <完整SHA> --json
wcode acceptance verify --base <完整SHA> --head <完整SHA> --json
wcode acceptance inspect --base <完整SHA> --head <完整SHA> --check
wcode acceptance history --json
```

`--check` 只有当前原生 Record 为 ready 才成功；历史不提供该开关。命令没有 Policy 自动激活或人工自批准功能。验证失败的 Record 保留真实失败，运行后还需要独立 reviewer／操作者流程时仍显示 needs_review。

## Record 与失败语义

Record 包含 Code／Design revision、原生 Git base/head/tree/index/dirty 绑定、Policy generation 与配置来源摘要、所选 Plan、风险、每个 required check 的独立事实、Evidence 元数据、结构化原因及下一步动作。执行时间保留在捕获元数据中；稳定内容摘要不因再次读取时间改变。

required、discovered、mapped 不等于 executed；Unknown、Unavailable、TimedOut、Skipped 不会变成 Pass。Receipt 只在完整 revision、Git、Policy、命令签名及足够验证等级全部匹配时用于当前判定。同文件内容的新 commit、Policy 重激活、撤销、过期、定义改变均使旧事实无法批准当前候选。模型评审和人工批准不能消除确定性失败。

状态包括 ready、blocked、needs_review、incomplete、stale。无数据、捕获失败或缓存快照不能凭计数推导 ready。TUI 与本地 Observatory 消费同一个原生 Record，保留高级图谱、源码与历史入口。

## 已安装二进制中的发布操作

现在可直接使用 `github preflight` 和 `github publish`，不需要在候选仓库里编译 `examples/git_publisher.rs`。经过审查的二进制、发布身份配置和权威状态必须位于不可信 Worker 无法写入的位置。CLI 会检查配置目录与候选目录不重叠，但不同目录本身并不构成操作系统沙箱或租户隔离。

```text
wcode github preflight --config-root /trusted/publisher-config --pull 17 --check --json
wcode -w /trusted/candidate github publish \
  --config-root /trusted/publisher-config --pull 17 \
  --workspace-id project-id --base <完整SHA> --head <完整SHA> --json
```

配置目录包含经过操作者审查的 `.wcode/github-publisher.yaml`，采用下文登记功能的同一无密钥格式。候选目录必须与配置目录分离。发布进程从可信启动环境读取 `WCODE_GITHUB_PUBLISHER_TOKEN`，不提供 token 参数、任意 API 地址参数或 proof JSON 入口。不要把凭据注入构建、测试、PR 脚本，也不要从不可信源码执行带凭据的 `cargo run`。静态 Token 模式仍由部署侧密钥管理器交付短期 installation 凭据，不自行续期；独立可信部署可显式启用下文的 App 私钥续期模式，两种来源不能混用。

预检通过有界 GET 读取仓库元数据；可选 App 私钥认证会另外换取安装令牌。仓库检查核对当前 PR 的仓库/base/head，以及绑定预期 App、要求最新 base、管理员也受约束的传统分支保护，或身份匹配、严格检查、可见且为空的 bypass actors 的生效规则集。实际生效规则必须在固定额度内完整分页；权限不足、API 故障、错误响应、分页过多或候选变化都会保留 incomplete。GitHub 在读取者缺少规则集写权限时可能不返回 bypass actors，缺失不能当作空列表。应让规则集操作者检查访问边界，不要为了让诊断变绿而扩大 Publisher 的管理权限。当前发布器不验证 merge-group commit，因此显式拒绝 merge queue。参阅 [规则 API](https://docs.github.com/en/rest/repos/rules)和[传统分支保护 API](https://docs.github.com/en/rest/branches/branch-protection)。

`configuration_verified` 只表示检查配置观察，不是当前 Acceptance、Check 写权限或部署隔离；对应独立字段保持 false。观察不完整时 `preflight --check` 非零退出。Publish 每次重新预检，要求明确事件 base/head SHA，拒绝旧事件而不是自动换成新候选，先写失败 Check，只有原生 Acceptance 重新捕获及精确回执检查通过才能写成功。它不补跑缺失验证、不激活 Policy、不批准评审、不改远端规则、不合并。`--read-only` 和 `--no-exec` 会在读取凭据或联网前拒绝发布。

可信控制器可在已验证的 PR 事件、原生验证完成或 Policy 变化后调用该命令；事件来源认证与仓库选择必须由可信部署完成，客户端字段不构成授权。重启或重试应重新预检和捕获，不能重放旧 receipt。API 故障不表示允许合并，部署还必须处理 Policy/证据改变后撤销既有成功；当前顺序 Check 协议不能保证 GitHub 故障时即时撤回。采用真实原生验证的本地 HTTP fixture 证明的是协议行为，不是客户部署或远端 required-check Pilot。

## 可选的安装令牌自动续期

独立可信 Publisher 可启用自动续期，不再因启动时的安装令牌到期而要求重启 watcher 或 inbox worker。Unix 优先使用私有文件模式：App 私钥放在候选与 inbox 之外的私有目录，由 Publisher 用户所有；文件和直接父目录均不能向组或其他用户开放。相对路径、符号链接及其父路径、硬链接、超过 32 KiB 的文件、加密或不支持的密钥均拒绝。其他受支持平台可由可信服务启动器通过 `WCODE_GITHUB_APP_PRIVATE_KEY` 直接注入 PEM 内容；wcode 只在内存中校验有界字节，不把它持久化。支持未加密 RSA PKCS#1／PKCS#8 PEM，并且必须且只能选择一种 App 私钥来源。

```sh
unset WCODE_GITHUB_PUBLISHER_TOKEN
export WCODE_GITHUB_APP_PRIVATE_KEY_FILE=/srv/wcode/private/app-signing-key.pem # Unix
# 或由可信服务环境直接注入 WCODE_GITHUB_APP_PRIVATE_KEY 的 PEM 内容。
export WCODE_GITHUB_INSTALLATION_ID=789
# 然后调用已有 preflight、publish、watch-candidate 或 work-inbox 命令。
```

安装 ID 来自可信启动配置，不由事件自行选择。事件／inbox 命令已有明确 installation ID 时，可以省略环境中的 ID；两者冲突则拒绝。App 配置和静态 Token 同时存在也会明确失败，不静默选用或回退。原有写入／执行权限、目录边界及原始事件验签仍先于私钥读取。Verify-event、接收器、状态和归档操作不会读取签名私钥。

续期用 RS256 签署 App JWT，实时核对指定仓库的 installation ID、App ID 和暂停状态，然后只为登记的数字 repository ID 请求令牌。权限固定为 Checks 写入，以及 Pull requests、Contents、Administration、Metadata 读取；响应包含其他仓库、额外或缺失权限、异常过期时间时均拒绝，不为隐藏的规则集信息自动扩大权限。只访问固定 GitHub HTTPS 地址，不跟随重定向；令牌作为有界不透明字符串处理，不假定固定 40 字符。参阅 [GitHub 安装令牌](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app)及 [App JWT 规则](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-json-web-token-jwt-for-a-github-app)。

同一 Provider 的并发请求共用一次换证。距过期不足 120 秒时由下一请求续期，同时校验墙上时钟和单调时钟，时钟回拨不能延长缓存。换证及等待限 35 秒，失败从 5 秒退避至最多 300 秒，不回退到已过期或临近过期的旧令牌。文件读取和 RSA 签名不占用异步执行线程，独立名额由底层真实工作持有至结束，不能因为等待方取消或超时就提前归还；文件模式每次续期重读受保护私钥，支持操作者预置新私钥；内存模式则固定使用进程启动时注入的 PEM。删除私钥文件并不等于远端撤销已签发令牌，缓存令牌仍可能使用至下一次续期。

收到 401 只使实际被拒绝的令牌失效，旧请求迟到的 401 不会误删新令牌。失败的 HTTP 请求不会自动重放，尤其不会重发结果不明的 Check 写入；后续仍交由既有 watcher／队列的验收与重试规则控制。续期失败只表示不可用，不会产生原生验证证据，也不能假报远端撤销成功。App 模式预检会报告 credential_renewal_enabled=true，并保守标记 remote_mutations=true，因为认证可能签发新令牌；repository_mutations 仍为 false。

App 私钥权限高于单仓库令牌，必须使用专用 App，并隔离候选代码、接收器和其他租户对签名宿主的访问。限制签发请求的权限，不等于限制被窃私钥本身的能力。该功能不创建远端 App、不授予权限、不安装 TLS 或操作系统／租户隔离，也不在 GitHub 上生成或轮换 App 私钥。尚未实现私有文件权限验证的非 Unix 平台明确拒绝 App 私钥模式，仍可使用外部管理的静态令牌。本地 RSA／HTTP 和 CLI 测试不是实际安装或客户部署验收。

## 固定候选的持续重新验收

`github watch-candidate` 补上发布后的重新检查，不重放已完成事件：

```text
wcode -w /trusted/candidate github watch-candidate \
  --config-root /trusted/publisher-config --pull 17 \
  --workspace-id project-id --base <完整SHA> --head <完整SHA> --json
```

它以前台方式运行，沿用 `publish` 的写入／执行权限和配置目录隔离检查，使用单独提供的 installation 凭据。每轮重新执行实时预检并捕获原生 Policy／Verification／Evidence。本进程首次创建失败 Check，此后复用同一个精确 ID。原生事实未变时仍重新捕获并核对实时 Check，但不重复写入；事实改变后继续走先失败、再原生复核和回执检查的发布路径。控制器不会补跑测试或自批：真实验证完成可让被阻止的候选变为 ready，Policy 撤销、证据改变或元数据不完整则移除 ready；仅恢复 Policy 不能让旧代次检查重新有效。

每轮结束后等待 30 秒；连续不可用时分别等待 60、120、最多 240 秒，不重叠、不突发补跑。这是调度间隔，不是最大撤销延迟。JSON Lines 只报告新观察或明确不可用，不把旧成功当成当前证明。PR 的 head/base 或保护配置变化时，只尝试把本进程原来的 Check 改为失败，不自动跟随新提交；更新前重验 Check 名称、App 与 SHA。底层操作使用 [Checks API](https://docs.github.com/en/rest/checks/runs#update-a-check-run)，并不构成跨系统合并事务。

Unix SIGINT／SIGTERM 在启动输出之前注册；停机先停止新轮询，等当前操作结束，再将自有 Check 改为失败并重新读取确认，之后才报告成功停止。输出故障也会尝试撤销。网络失败、凭据无效或 Check 身份不匹配时明确报告撤销未确认，之前的远端成功可能仍可见；强制杀进程无法执行安全停机。部署须为每个绑定保证单个受监督发布者，不要让 inbox worker、单次发布命令和 watcher 同时写同一候选。重启重新创建受保护 Check，不导入历史 ID。可信 supervisor 仍负责新候选选择、签名私钥保管或外部静态令牌续期、配置变化和崩溃恢复；本命令不安装后台服务、TLS、分支规则或租户隔离，真实远端部署仍须另行验收。

### 同状态根的发布进程协调

共享受保护原生状态根的合作发布进程，现在通过非阻塞操作系统锁协调同一 API、数字 repository ID、预期 App、Check 名称和 head SHA。这些进程应使用同一个 `WCODE_STATE_DIR`；不同本地 checkout 标签、配置目录、凭据、PR 编号或 base，不会为相同 Check/head 绕出另一个名额。Watcher 在轮询间隔及停机撤销期间持续持有名额，单次发布则持有至实际操作结束。包括底层 provider 接口在内，每次 Check 写入都要求匹配的内部 guard；竞争者明确返回 busy，不写 Check。只读预检、inbox 状态和接收请求不需要这个发布名额。

Inbox 在消耗尝试次数之前取得候选名额。正在被占用的事件保留状态、次数和退避期限，有界到期扫描可继续处理其他已经验签的候选；全部候选不可用时明确报错，不能假装空闲或耗尽重试。名额一直保留到最后的持久状态写入结束，中断恢复及防重放规则不变。

`github-publication-locks` 下的稳定空文件只用于协调，不保存凭据、证明或 Check ID。Guard 显式解锁，不截断、不删除，也不按超时抢占；不要靠删除活动锁文件解除占用，否则可能出现两份独立 inode。Unix 会检查私有权限、路径别名、硬链接及持有／命名 inode 身份；Windows ACL 部署验收仍须另做。进程死亡只释放本地名额，不会撤销 GitHub 已经收到的请求；不同状态根、主机、旧二进制及不遵守该协议的发布者仍需外部协调，不能把它宣称为原子合并锁或远端回滚。参阅 [Rust 文件锁语义](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)。

## 发布前验证 PR 事件来源

工作树二进制新增 `github verify-event` 与 `github publish-event`。可信接收器通过 stdin 传入未经修改的 JSON 请求体；共享库先对 `X-Hub-Signature-256` 做恒定时间 HMAC-SHA256 校验，再解析 PR 身份。单独配置 16–4,096 字节的高熵 webhook 密钥，不能复用发布 API Token。超过一 MiB 的请求、缺失或重复头、错误签名、安装或仓库身份不匹配、已关闭／草稿／已合并 PR、不支持的 action 和不完整 SHA 均拒绝。支持 opened、reopened、synchronize、ready_for_review、edited；fork head 仍须绑定登记的 base 仓库和完整候选身份。

```text
wcode github verify-event --config-root /trusted/publisher-config \
  --installation-id 789 --json < /trusted/inbox/raw-delivery.json
wcode -w /trusted/candidate github publish-event \
  --config-root /trusted/publisher-config --installation-id 789 \
  --workspace-id project-id --json < /trusted/inbox/raw-delivery.json
```

可信接收器在启动环境中提供 `WCODE_GITHUB_WEBHOOK_SECRET`，并用 `WCODE_GITHUB_SIGNATURE_256`、`WCODE_GITHUB_EVENT`、`WCODE_GITHUB_DELIVERY`、`WCODE_GITHUB_CONTENT_TYPE` 传递原始头；不提供 CLI 密钥参数。接收器必须在合并为环境变量之前拒绝重复头，关闭请求体管道，并约束输入及整体执行时限。配置目录、安装 ID、候选路径由可信路由配置选定，不能接受事件自行指定。签名验证必须使用原始 UTF-8 字节，不能先把 JSON 解析后重新序列化。参阅 [GitHub 签名验证](https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries)和[事件接收最佳实践](https://docs.github.com/en/webhooks/using-webhooks/best-practices-for-using-webhooks)。

Verify-event 不联网、不写入状态，只返回有界身份与请求体 SHA-256 摘要，不回显原始 body、标题或发送者文本。共享密钥签名有效不等于事件新鲜、人工批准、原生验证或防重放，对应字段保持 false。Delivery-ID 头不在请求体签名中，因此仅替换头不能改变 body digest 或制造新工程授权。原始 stdin 命令本身不提供持久去重和传输时限。下文 inbox 命令增加持久接收及有界 loopback HTTP 入口；TLS 与可信连接／请求头时限仍由部署负责。

Publish-event 先拒绝只读／禁执行及配置目录重叠，再认证事件，最后才读取独立的 API 凭据。随后重新执行既有远端预检和原生捕获，使用签名请求中的精确 base/head；旧事件被拒绝，不会自动改投新提交。事件中的 ready、PASS、sender role、human approval 等字段均不影响验收。验签不能补齐缺失验证、批准评审，也不能保证 API 故障时撤回旧成功；本地签名事件和真实原生验证测试不等于远端客户 Pilot。

## 持久接收与重试

Inbox 必须是与候选代码分离的新私有目录，父目录须已存在，绝对路径不得包含符号链接。配置中的仓库、数字 repository／App／installation ID、Check 名称和 inbox 根均被绑定；已有、缺失或损坏的 store 不会被静默重建。

```text
wcode github inbox-init --config-root /trusted/publisher-config \
  --installation-id 789 --inbox /trusted/deliveries --json
wcode github serve-inbox --config-root /trusted/publisher-config \
  --installation-id 789 --inbox /trusted/deliveries --listen 127.0.0.1:8788
wcode github inbox-status --config-root /trusted/publisher-config \
  --installation-id 789 --inbox /trusted/deliveries --json
wcode -w /trusted/candidate github publish-next \
  --config-root /trusted/publisher-config --installation-id 789 \
  --inbox /trusted/deliveries --workspace-id project-id --json
```

Serve-inbox 是前台进程，只接收 `POST /github/webhook`，使用独立 webhook 密钥，不读取发布 API Token。对外部署须使用 HTTPS 反向代理，保留原始请求体和重复头，并约束连接数、请求头时间和速率。入口允许两个请求，每份最多一 MiB，请求体限五秒，响应预算八秒。签名／路由失败返回 401；过载、存储冲突或持久化未确认返回 503，绝不报告接收成功；只有确认落盘后返回 202。存储操作不阻塞异步执行器，即使响应超时也要等真实存储工作结束才归还容量，不能无限创建阻塞任务。失败请求须由监督集成明确重投，不能把未确认接收当成功。参阅 [GitHub delivery best practices](https://docs.github.com/en/webhooks/using-webhooks/best-practices-for-using-webhooks)。

需要自动消费时，从固定安装启动独立的前台工作进程：

```text
wcode -w /trusted/candidate github work-inbox \
  --config-root /trusted/publisher-config --installation-id 789 \
  --inbox /trusted/deliveries --workspace-id project-id --json
```

工作进程每次只处理一个到期事件；成功或空轮询后间隔一秒，不可用轮询后间隔五秒，仍严格遵守事件原有的更长退避时间和次数上限。慢操作结束后不会突发补跑。JSON Lines 输出启动、真实发布结果、脱敏错误和停机摘要；空轮询不刷屏，计数不代表当前 Acceptance。Unix 上 SIGINT／SIGTERM 停止新轮询，并等待当前发布及状态写入完成；非 Unix 使用 Ctrl+C。服务管理器须留出安全停机时间，强制杀进程仍可能留下中断及远端结果不明的请求。命令不自动安装或后台化服务；配置或密钥变更需要受控重启，验证／Policy 改变后也不会自动重放已完成事件。参阅 [Tokio 安全停机](https://tokio.rs/tokio/topics/shutdown)。

已有可信接收器也可调用 `github enqueue-event`，传入同样的 config／installation／inbox 参数，并按上文通过 stdin 与环境传递原始签名请求。入队不访问 API 凭据。Inbox 私有保存原始请求和签名头以便重试时重新验签，使用 base64 编码，**不是加密**；两种密钥均不落盘，status 也不返回 body 或签名。

完整 inbox 依据签名请求体的 digest 去重，覆盖重投、替换未签名 Delivery-ID 及进程重启。短暂的状态锁保护原子快照写入，独立 worker 锁覆盖一次发布，但不会锁住入队和状态查询。保护对象会在操作结束时显式解锁，避免复制的文件描述符让已结束操作仍显示忙。不会使用到期租约抢占仍在运行的请求。中断后的 `publishing` 保留可见，到期再重试；每次都用当前密钥重新验签、读取实时预检、精确候选和原生 Acceptance。退避分别为 10／20／40／80／160 秒，最多五次；API 故障或本地完成未确认不产生成功回执。

密钥变更后，无法验签的队首不会挡住后续合法事件：worker 在有界到期集合中寻找首个能用当前密钥验证的事件，旧密钥事件原样保留。如果所有到期事件都无法验签，明确返回不可用，而不是假装队列为空或消耗发布重试次数；长度无效的 worker 密钥在恢复写入之前就被拒绝。

收到用当前密钥签名、且**原始请求体完全相同**的重投后，可以只刷新 queued／retry 项中已经失效的签名头。回执保留 `duplicate=true`，并通过 `reauthenticated=true` 标识本次恢复；新快照确认持久化后才推进 generation。原始字节、摘要、首次接收时间、尝试次数和重试状态保留，退避期限不会提前。同一有效签名再次到达就是普通重复请求，不再改状态。completed、exhausted 和正在 publishing 的事件不能借此重置。系统不会自动生成、接受请求中自带的密钥、安装或轮换密钥，也不假定 GitHub 重投必然使用新密钥：实际未能通过当前密钥验证的旧事件仍需操作者核对。这是队列认证恢复，不是事件新鲜度或 Acceptance 证明。

快照最多八 MiB、128 个事件，已完成事件的去重记录也计入上限；满载明确报错，不偷偷删历史绕过去重。队列 completed 只表示该事件处理已结束，也可能对应被阻止的 Check，不代表当前验收通过，更不会重放旧绿色回执。之后验证／Policy 变化可由单独监督的 `github watch-candidate` 持续检查，或显式重新调用 `github publish`，不能靠重复已完成事件更新。控制器还须检查 exhausted、管理可信归档、轮换密钥前清理或核对未决事件、监督前台 worker；命令不会自动安装服务或静默丢弃异常事件。

远端效果不保证恰好一次：中断请求可能已经到达 GitHub；重试重新验证，不能声称远端已回滚。校验和只检测损坏，不能抵御同 UID 恶意修改或回滚到旧的完整快照。Unix 私有权限和路径检查不建立 Windows ACL、租户隔离、TLS 或外部防回滚 checkpoint；部署隔离与真实远端 Pilot 仍需独立验收。

## 显式归档已结束事件

`github archive-inbox` 可以释放 completed／exhausted 事件的请求体槽位，但不遗忘其防重放身份。它要求有效的已有 inbox、写权限，以及 `inbox-status` 返回的精确 generation；不读取 API／webhook 密钥，也不访问 GitHub。归档前应停止新消费并排空 inbox worker：已有 worker 持锁时会明确拒绝，不中断发布。独立 watcher 的锁和只读诊断不受此操作阻塞。

```text
wcode github archive-inbox --config-root /trusted/publisher-config \
  --installation-id 789 --inbox /trusted/deliveries \
  --expected-generation 42 --output /private/archives/deliveries-42.json --json
wcode github inspect-inbox-archive --config-root /trusted/publisher-config \
  --installation-id 789 --inbox /trusted/deliveries \
  --archive /private/archives/deliveries-42.json --digest <独立保管的sha256摘要> --read-only --json
```

输出文件必须全新、绝对路径、位于 inbox 之外，父目录已存在且为无别名私有目录。归档保留原始签名请求体、请求头及结束状态，**内容未加密，必须按机密数据保管**。完整归档同步并按摘要读回确认后，才会压缩在线快照；标准输出只有摘要／字节数、前后 generation、根身份和计数。检查点应独立保管。检查命令核对外部摘要、登记身份和原始记录，不导入记录，也不输出请求体或签名。

压缩后，最多 128 个在线请求体记录之外，还可保留 16,384 个紧凑的终态防重放记录；整个快照仍受八 MiB 总上限约束，status 分开报告两种计数。queued、retry 和中断的 publishing 事件不会归档。旧事件重投仍先验签，再返回 `duplicate=true, archived=true`，不重置次数、不重新发布；外部归档丢失也不会删除在线防重放身份。紧凑索引满载仍明确拒绝，不偷偷丢历史；这不是无限保留的承诺。

未经归档的版本 1 快照保持原来的校验和表示。首次显式归档写入带防重放索引的版本 2；旧二进制必须拒绝，不能用它降级或丢弃新索引。归档文件与队列快照不是跨文件原子事务：导出失败时不压缩队列，但可能留下只创建不覆盖的部分文件；最终快照写入确认失败时应检查实际 generation，不假称回滚或盲目重试。归档、旧快照和检查点都不能恢复当前 Acceptance。加密、异地保管、独立检查点保护、Windows ACL 和跨机器恢复仍须部署验收。

## 持久化与外部 gate

本地历史在受保护状态根的 `acceptance-history` 中，限制 256 条，每条不超过 2 MiB；容量或损坏显式报错。内容摘要用于发现损坏，不是签名，也不抵御同 OS 用户下任意恶意代码。重启后历史仍为 historical_only。

OSS Git provider adapter 位于 `src/integrations/git/`。项目只需显式登记一次不含密钥的发布身份：

```text
wcode setup --project \
  --github-repository owner/repo \
  --github-repository-id <数字 repository ID> \
  --github-app-id <数字 App ID>
```

登记操作在正常的项目 Host 配置与 Design 初始化之外，新增 `.wcode/github-publisher.yaml` 中的 repository／App／Check 身份。无效身份、纯空白 Check 名称、已损坏的 manifest 或身份冲突，会在这些 setup 写入之前被拒绝；即使 manifest 不存在，父目录也必须通过 Workspace 路径检查。最终登记写入会再次检查当前状态、不覆盖其他身份，但 setup 不提供多文件原子事务。该过程不保存 Publisher Token，也不会修改 GitHub 远端设置；`--dry-run --json` 只预览、不落盘。真实发布示例 `examples/git_publisher.rs` 会读取这份 enrollment，Publisher 凭据仍从隔离发布环境单独取得。公开发布方法自行重新捕获原生 Acceptance，校验 PR base/head、repository ID、Check 来源 App 和发布回执；不会接收 Worker 的 PASS JSON，也不执行仓库命令。隔离部署必须把 Publisher／Policy／凭据放在不可信 Worker 无法写入或访问的位置。

GitHub 的 required checks 可接受 neutral／skipped；wcode adapter 只认可 expected App、exact SHA 且 completed/success。配置 branch protection 时必须约束该 Check 名称及来源 App。参阅 [GitHub 分支保护](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)与 [Checks API](https://docs.github.com/en/rest/checks/runs)。

GitHub API 故障使本次发布失败；它不能追溯撤销同 SHA 上已有的成功，也不提供远端 PR、Policy 与本地文件之间的原子事务。真实远端 PR Pilot、受保护部署和组织级治理必须独立验收，不能把协议测试描述成已部署。
