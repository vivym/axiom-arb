结论先放前面：**最佳实现不是“一个 Python 脚本不停轮询下单”，而是“Python 做研究/回测，Rust 做生产执行”的事件驱动架构**。原因是 Polymarket 现在是 **hybrid-decentralized CLOB**：撮合在链下，成交在 Polygon 上原子结算；下单要做 EIP-712 签名；交易接口分公开数据接口和带 L2 凭证的私有交易接口；实时数据主要走 `market`/`user` WebSocket；并且你还要处理 split / merge / redeem 这类 CTF 库存操作。这样的系统天然更适合做成“本地状态机 + 实时风控 + 执行引擎”，而不是同步脚本。官方也提供了 Python、TypeScript、Rust 三套 CLOB SDK。([Polymarket 文档][1])

## 我建议的“最佳套利方向”

**第一优先级：同市场 full-set arbitrage。**
这是 Polymarket 上最干净、最接近真套利的一类。因为 Polymarket 的二元市场本质上是 YES/NO 两个 outcome token，而且官方 CTF 支持：`$1 USDC.e -> 1 YES + 1 NO` 的 split，以及 `1 YES + 1 NO -> $1 USDC.e` 的 merge。于是当你发现 **best ask(YES) + best ask(NO) + 全部成本 < 1** 时，可以买两边然后 merge；反过来，当 **best bid(YES) + best bid(NO) - 全部成本 > 1** 时，可以先 split 出 full set，再把 YES/NO 分别卖掉。这个方向最适合先做，因为逻辑清晰、风险边界最明确。([Polymarket 文档][2])

**第二优先级：negative-risk 事件套利。**
Polymarket 官方文档明确有 `Negative Risk Markets`：在这类互斥多结果事件中，“某个 outcome 的 1 张 No”可以通过 Neg Risk Adapter 转成“其他 outcomes 各 1 张 Yes”。这会带来跨 outcome 的结构性价差机会。这个方向往往比 full-set 更赚钱，但实现复杂度更高，因为你不只是在一个 binary market 上算，还要维护整个 neg-risk 事件的转换关系、路径成本和库存净额。([Polymarket 文档][3])

**第三优先级：maker-rebate / liquidity-reward 增强型套利。**
Polymarket 现在有 maker rebates 和 liquidity rewards，部分市场的 maker 会分到 taker fee 的一部分，流动性奖励按规则每日发放。它们能把“微弱负 edge”翻成正 edge，但这不属于纯套利，因为你会暴露在被动成交、价格漂移和 adverse selection 里。所以它应该是 **增强器**，不是主策略。([Polymarket 文档][4])

**第四优先级：逻辑相关市场套利。**
比如父子事件、互斥事件、体育比赛中不同时间窗市场之间的不一致。这类机会很多，但严格说更像“结构化统计套利”，不是无风险套利。它适合放到第二阶段，不要作为第一版 bot 的核心。

---

## 最优系统架构

我建议做成下面这个结构：

```text
Market Discovery / Metadata
    └─ Gamma API / Data API / Subgraph

Feed Handler
    ├─ market WebSocket
    ├─ user WebSocket
    └─ REST snapshot & reconciliation

Local State Engine
    ├─ orderbooks
    ├─ positions / balances
    ├─ open orders
    ├─ tick size / market status
    └─ event lifecycle

Opportunity Engine
    ├─ full-set arb
    ├─ neg-risk arb
    ├─ cross-market logical arb
    └─ reward-adjusted maker arb

Execution Engine
    ├─ place / cancel / replace
    ├─ batch orders
    ├─ split / merge / redeem
    └─ retry / idempotency

Risk Engine
    ├─ exposure limits
    ├─ stale-data kill switch
    ├─ event-start / resolution guard
    ├─ fee / slippage checks
    └─ restart / rate-limit handling

Persistence & Observability
    ├─ append-only event journal
    ├─ PnL / inventory accounting
    ├─ replay / backtest
    └─ alerts / dashboards
```

这个拆法是因为官方建议 **实时 orderbook 用 WebSocket，不要靠轮询**；`market` channel 会推 orderbook、price、trade 和市场生命周期更新，`user` channel 会推你自己的 order placement / update / cancellation / trade；另外用户 WebSocket 还要求客户端定期 heartbeat。你需要一个本地状态层来把这些实时流和 REST 快照对齐，否则你的机会检测和库存计算会经常错。([Polymarket 文档][5])

---

## 技术选型

**生产执行层：Rust。**
原因不是“Rust 更潮”，而是这个 bot 的核心瓶颈在于：并发 WebSocket、订单状态机、库存一致性、断线恢复、批量撤改单、精确数值处理。Rust 在这些点上更稳。官方已经有 Rust CLOB client，所以接入成本并不高。([Polymarket 文档][6])

**研究 / 回测层：Python。**
策略研究、机会扫描、参数搜索、PnL 分解，用 Python 最快。你甚至可以先用官方 Python client 做一个 shadow bot，验证机会频率和成交质量，再把生产执行迁到 Rust。官方也有 Python SDK。([GitHub][7])

**存储：Postgres 为主，Redis 可选。**
单 bot 场景下，别一开始上 Kafka。最实用的做法是：

* Postgres 存事件日志、订单日志、fill、持仓、PnL
* 内存里维护热状态
* Redis 只在你需要跨进程共享热数据时再加
  这会比“微服务 + 消息总线”简单很多，也更容易排障。

---

## 执行层该怎么写

Polymarket 虽然支持“市场单”，但官方定义其实仍然是 **marketable limit order**；订单类型核心是 GTC / GTD / FOK / FAK。我的建议是：

* **maker quote** 用 `GTC` 或 `GTD + postOnly`
* **抢 arb** 用 `FAK`
* **必须整笔吃到才有 edge** 的用 `FOK`
* **已知事件时间点前** 一律用 `GTD` 自动过期，不要赌自己一定能来得及撤单。
  官方还支持一次最多提交 15 个预签名订单，这对跨腿同时下单很重要。([Polymarket 文档][8])

你本地的订单状态机至少要有这几个状态：
`IntentCreated -> Signed -> Submitted -> Acked -> PartiallyFilled -> Filled / CancelPending / Cancelled / Rejected`
而且必须把 **下单成功** 和 **最终成交** 分开记。Polymarket 这类系统里，很多 bug 都不是“没下出去”，而是“你以为自己没成交，其实成交了；或你以为撤掉了，其实还挂着”。

---

## 库存与资金层

这是 Polymarket bot 最容易被低估的部分。很多人只盯着 orderbook，却没认真做 inventory engine。实际上 Polymarket 官方专门把 market maker 的 inventory management 拆出来讲了：你需要管理 **USDC.e、YES、NO、split、merge、redeem**。而且这些 CTF 操作可以通过 relayer 走 gasless 流程。对套利 bot 来说，这意味着你不应该只把自己看成“下单器”，而要把自己看成“资金和 token 变换引擎”。([Polymarket 文档][9])

我建议把钱包拆成两层：

* **hot trading wallet**：小额、专门跑 bot
* **treasury wallet**：大额、冷一些，只做周期性补充
  这样一旦 bot 出错，损失不会直接打穿全仓。

---

## 风控上最关键的几个点

**1）手续费和奖励必须进 edge 公式。**
Polymarket taker fee 不是固定百分比，而是跟价格有关；文档还明确给了公式，而且有效费率在 50% 概率附近最高。maker rebate 也因市场类型而异。所以你的机会判断不能只看 `yes+no`，必须看 **净边际收益**。([Polymarket 文档][10])

**2）事件生命周期风控必须单独做。**
文档明确写了：市场 resolution 后，交易停止，winning token 可按 `$1` 赎回，losing token 归零。体育市场的挂单会在比赛开始时自动取消，但官方也提醒了：如果比赛比预定时间更早开始，系统清单可能来不及。因此体育类 bot 不能依赖平台帮你兜底，必须自己在事件前收紧或撤掉风险单。([Polymarket 文档][11])

**3）处理平台维护与速率限制。**
官方文档写明：matching engine 每周二美东时间 7:00 会重启，订单相关接口在窗口期会返回 HTTP 425；API 还有 Cloudflare 限流，通用限额是 15,000 req / 10s。生产 bot 必须有 `425` 专门分支、退避重试、以及客户端 rate limiter。([Polymarket 文档][12])

**4）动态 tick size。**
官方 market channel 文档提到在价格非常靠近 0 或 1 时，tick size 会变化。你本地报价器如果不处理这个，很容易报出非法价位，或者算错成交边界。([Polymarket 文档][13])

**5）合规。**
Polymarket 文档明确提到有 geographic restrictions，builder 在下单前需要检查是否受地域与制裁规则限制。这个要在系统设计里当成硬约束，不是上线前再补的东西。([Polymarket 文档][14])

---

## 我认为最好的落地路线

**Phase 1：先只做 full-set arb。**
只做一个策略：

* 订阅 market WS
* 维护本地 best bid / ask
* 算 `ask_yes + ask_no + costs < 1`
* 或 `bid_yes + bid_no - costs > 1`
* 接上 split / merge
* shadow 模式跑 3–7 天，先测机会频率和净 edge

**Phase 2：加上 neg-risk。**
先挑 1 个事件族做，不要一上来全市场扫。因为 neg-risk 的难点不是下单，而是组合状态管理。

**Phase 3：再做 maker-enhanced 版本。**
这时再把 rebates / liquidity rewards 算进去，做被动挂单 + 被动库存再平衡。

---

## 一句话版推荐

**最优解 = Rust 单体执行服务 + Python 研究层 + WebSocket 驱动本地状态机 + full-set arb 先行 + inventory/risk engine 作为一等公民。**
不要先做“大而全多策略平台”，也不要先做“只会扫价差的 Python 脚本”。在 Polymarket 上，真正决定 bot 生死的，通常不是策略公式本身，而是 **本地状态是否准、库存是否准、撤改单是否稳、生命周期风控是否硬**。

[1]: https://docs.polymarket.com/trading/overview?utm_source=chatgpt.com "Overview - Polymarket Documentation"
[2]: https://docs.polymarket.com/trading/ctf/split?utm_source=chatgpt.com "Split Tokens"
[3]: https://docs.polymarket.com/advanced/neg-risk?utm_source=chatgpt.com "Negative Risk Markets"
[4]: https://docs.polymarket.com/market-makers/liquidity-rewards?utm_source=chatgpt.com "Liquidity Rewards"
[5]: https://docs.polymarket.com/trading/orderbook?utm_source=chatgpt.com "Orderbook - Polymarket Documentation"
[6]: https://docs.polymarket.com/api-reference/clients-sdks?utm_source=chatgpt.com "Clients & SDKs"
[7]: https://github.com/Polymarket/py-clob-client?utm_source=chatgpt.com "Python client for the Polymarket CLOB"
[8]: https://docs.polymarket.com/trading/orders/create?utm_source=chatgpt.com "Create Order"
[9]: https://docs.polymarket.com/market-makers/inventory?utm_source=chatgpt.com "Inventory Management"
[10]: https://docs.polymarket.com/trading/fees?utm_source=chatgpt.com "Fees"
[11]: https://docs.polymarket.com/concepts/resolution?utm_source=chatgpt.com "Resolution"
[12]: https://docs.polymarket.com/trading/matching-engine?utm_source=chatgpt.com "Matching Engine Restarts"
[13]: https://docs.polymarket.com/cn/market-data/websocket/market-channel?utm_source=chatgpt.com "Market 频道"
[14]: https://docs.polymarket.com/api-reference/geoblock?utm_source=chatgpt.com "Geographic Restrictions"

可以。下面我直接给你一版**能开工的工程方案**：项目名、repo 结构、核心模块接口、数据库表、订单状态机，以及 v1 该做什么、不该做什么。

## 先定一个推荐名字

我最推荐：

**AxiomArb**

原因：

* `Axiom` 很适合“市场内部应满足的定价公理 / 一致性约束”
* 不把 `Polymarket` 写进 repo 名，后面扩到别的 prediction market 也不别扭
* 听起来像基础设施，不像一次性脚本

我会这样命名：

* 仓库名：`axiom-arb`
* 生产主程序：`axiomd`
* v1 套利策略二进制：`fullsetd`
* 回测/重放工具：`axiom-replay`
* Python 研究目录：`research/`

如果你想更直白一点，第二选择是：

**FullSet**

* 仓库名：`fullset`
* 非常贴合你第一阶段最该做的 full-set arbitrage
* 缺点是后面扩到 neg-risk、maker rebate 时名字会略窄

我的建议是：**项目名用 AxiomArb，第一版策略模块叫 fullset。**

---

## 最优工程形态：不是微服务，而是“单进程多任务 + 强事件日志”

v1 不要上微服务。
**最佳形态是 Rust monorepo，单个 live engine 进程里跑多个 async task**，外加一个 Python research 目录。

原因很简单：

* Polymarket 官方有 **Rust / Python / TypeScript** 三套官方 SDK，生产执行直接接 Rust 很顺手。([Polymarket 文档][1])
* 你必须同时处理 market WS、user WS、订单生命周期、库存、split/merge/redeem、重连、对账，这些在单进程内存态里更稳。
* 真正重要的是**可重放**，不是“服务拆得漂亮”。

---

## 推荐 repo 结构

```text
axiom-arb/
├─ Cargo.toml
├─ rust-toolchain.toml
├─ .env.example
├─ docker-compose.yml
├─ Makefile
├─ README.md
│
├─ crates/
│  ├─ app-live/                 # 生产主程序 axiomd
│  ├─ app-replay/               # 市场数据 / 用户事件重放
│  ├─ app-backfill/             # 元数据、历史订单、历史成交补采
│  │
│  ├─ common/                   # 通用类型、错误、Decimal、时间
│  ├─ config/                   # 配置加载
│  ├─ ids/                      # MarketId / AssetId / OrderId / ConditionId
│  │
│  ├─ venue-polymarket/         # Polymarket 适配层
│  ├─ feed/                     # WS / REST 拉流与快照对账
│  ├─ state/                    # 本地 orderbook / positions / balances / open orders
│  ├─ journal/                  # append-only event journal
│  ├─ persistence/              # Postgres DAO / migrations runtime
│  │
│  ├─ strategy-core/            # Strategy trait, Opportunity trait
│  ├─ strategy-fullset/         # v1: YES/NO full-set arb
│  ├─ strategy-negrisk/         # v2: neg-risk arb
│  ├─ strategy-maker/           # v3: reward / rebate adjusted maker quoting
│  │
│  ├─ pricing/                  # fee/slippage/net edge 计算
│  ├─ inventory/                # split / merge / redeem / balance reservation
│  ├─ execution/                # 下单 / 撤单 / replace / 批量下单
│  ├─ risk/                     # 风控、kill switch、时钟/事件状态防线
│  ├─ scheduler/                # 定时任务、对账、心跳、恢复
│  └─ observability/            # metrics / tracing / alerts
│
├─ migrations/
│  ├─ 0001_init.sql
│  ├─ 0002_orders.sql
│  ├─ 0003_positions.sql
│  └─ ...
│
├─ research/
│  ├─ pyproject.toml
│  ├─ notebooks/
│  ├─ datasets/
│  ├─ adapters/
│  ├─ backtests/
│  └─ reports/
│
└─ docs/
   ├─ architecture.md
   ├─ order-state-machine.md
   ├─ runbook.md
   └─ strategy-fullset.md
```

### 为什么这么拆

`venue-polymarket` 只做“平台适配”，别把策略写进去。
因为 Polymarket 有：

* CLOB 下单
* market / user WebSocket
* up to **15** 单 batch orders
* neg-risk 订单参数
* tick size 约束与 `tick_size_change`
* CTF split / merge / redeem
* user WS heartbeat
* matching engine restart 时的 `425` 处理
  这些都属于“venue 语义”，不该污染策略层。([Polymarket 文档][2])

---

## 进程内部怎么跑

我建议 live engine 里就 8 个核心 task：

1. **metadata task**
   拉市场元数据、token、condition、negRisk 标记、tick size 初值

2. **market feed task**
   订阅 `market` WS，吃 `book` / `price_change` / `last_trade_price` / `tick_size_change` / lifecycle 事件，维护本地 orderbook。官方 market channel 就是这么提供的。([Polymarket 文档][3])

3. **user feed task**
   订阅 `user` WS，接订单更新和成交回报；客户端要发 heartbeat，官方文档写的是**每 10 秒一次**。([Polymarket 文档][4])

4. **state reconciler**
   定时 REST snapshot 对账，纠正 websocket 丢包 / 重连后的偏差

5. **strategy task**
   从 state 读 best bid/ask、库存、fee params，产出机会

6. **risk task**
   对每个机会做净边际检查、库存检查、事件时间检查、平台状态检查

7. **execution task**
   下单 / 撤单 / 批量下单 / split / merge / redeem

8. **journal task**
   把所有外部事件和内部命令写成 append-only log，保证可重放

---

## v1 的最优策略边界

### 只做这个：

**full-set arbitrage**

#### 场景 A：买入后 merge

当：

```text
best_ask(YES) + best_ask(NO) + trading_cost + merge_cost < 1
```

就：

1. 同时买 YES / NO
2. 等成交确认
3. merge 成 USDC.e

#### 场景 B：split 后卖出

当：

```text
best_bid(YES) + best_bid(NO) - trading_cost - split_cost > 1
```

就：

1. 先拿 USDC.e split 成 YES + NO
2. 同时卖出 YES / NO

这套逻辑成立的根基，是 Polymarket 的 CTF 支持 split / merge / redeem：

* split：`$1 -> 1 YES + 1 NO`
* merge：`1 YES + 1 NO -> $1`
* resolution 后 winning token 可按 `$1` redeem，losing token 归零。([Polymarket 文档][5])

### 先不要做这个：

* 跨事件逻辑套利
* 多市场统计套利
* maker reward 强化做市
* 复杂 inventory recycling
* 任何需要预测 alpha 的东西

---

## Rust 核心接口怎么设计

### 1) 平台适配接口

```rust
pub trait Venue {
    type Error;

    async fn connect_market_ws(&self) -> Result<(), Self::Error>;
    async fn connect_user_ws(&self) -> Result<(), Self::Error>;

    async fn fetch_market_metadata(&self) -> Result<Vec<MarketMeta>, Self::Error>;
    async fn fetch_open_orders(&self) -> Result<Vec<VenueOrder>, Self::Error>;
    async fn fetch_balances(&self) -> Result<Vec<Balance>, Self::Error>;

    async fn place_order(&self, req: PlaceOrderReq) -> Result<PlaceOrderAck, Self::Error>;
    async fn place_orders(&self, reqs: Vec<PlaceOrderReq>) -> Result<Vec<PlaceOrderAck>, Self::Error>;
    async fn cancel_order(&self, order_id: VenueOrderId) -> Result<(), Self::Error>;
    async fn cancel_orders(&self, order_ids: Vec<VenueOrderId>) -> Result<(), Self::Error>;
}
```

这里 `place_orders` 一定要有，因为 Polymarket 官方支持**最多 15 单 batch**，这对双腿套利非常关键。([Polymarket 文档][2])

---

### 2) CTF / 库存接口

```rust
pub trait InventoryOps {
    type Error;

    async fn split(&self, condition_id: ConditionId, amount: Decimal) -> Result<TxReceipt, Self::Error>;
    async fn merge(&self, condition_id: ConditionId, amount: Decimal) -> Result<TxReceipt, Self::Error>;
    async fn redeem(&self, condition_id: ConditionId) -> Result<TxReceipt, Self::Error>;

    fn available_usdc(&self) -> Decimal;
    fn available_shares(&self, asset_id: AssetId) -> Decimal;

    fn reserve_usdc(&mut self, amount: Decimal) -> Result<ReservationId, Self::Error>;
    fn reserve_shares(&mut self, asset_id: AssetId, amount: Decimal) -> Result<ReservationId, Self::Error>;
    fn release_reservation(&mut self, id: ReservationId);
}
```

库存系统不要只看“钱包余额”，还要看：

* 开单占用
* 已成交未 merge
* 已 split 未卖出
* 已 resolved 未 redeem

---

### 3) 策略接口

```rust
pub trait Strategy {
    fn name(&self) -> &'static str;

    fn on_market_event(
        &mut self,
        event: &MarketEvent,
        state: &StateView,
    ) -> Vec<Opportunity>;

    fn on_user_event(
        &mut self,
        event: &UserEvent,
        state: &StateView,
    ) -> Vec<Opportunity>;

    fn on_timer(
        &mut self,
        now: chrono::DateTime<chrono::Utc>,
        state: &StateView,
    ) -> Vec<Opportunity>;
}
```

`Opportunity` 不直接变成“下单”，中间必须过 risk。

---

### 4) 风控接口

```rust
pub trait RiskEngine {
    fn evaluate(
        &self,
        opp: &Opportunity,
        state: &StateView,
    ) -> RiskDecision;
}

pub enum RiskDecision {
    Approve(ExecutionPlan),
    Reject { reason: RejectReason },
}
```

---

### 5) 执行计划接口

```rust
pub enum ExecutionPlan {
    FullSetBuyThenMerge {
        yes_asset: AssetId,
        no_asset: AssetId,
        yes_price_limit: Decimal,
        no_price_limit: Decimal,
        size: Decimal,
    },
    FullSetSplitThenSell {
        condition_id: ConditionId,
        yes_asset: AssetId,
        no_asset: AssetId,
        yes_price_limit: Decimal,
        no_price_limit: Decimal,
        size: Decimal,
    },
    CancelStale {
        order_ids: Vec<VenueOrderId>,
    },
    RedeemResolved {
        condition_id: ConditionId,
    },
}
```

关键点：**执行计划必须是“业务语义计划”，不是 API 调用序列。**
这样后面你换实现、做重试、做 replay 都轻松。

---

## 订单与执行的内部原则

### 定价与数值

代码里不要用浮点。
统一用：

* Rust: `rust_decimal::Decimal`
* DB: `NUMERIC(38,18)`

价格虽然在 0~1，但 tick size 可能变化，而且官方 market channel 会推 `tick_size_change`，订单价格不符合 tick size 会被拒。([Polymarket 文档][3])

### 订单类型

你内部要明确区分：

* `GTC` / `GTD`：挂在簿上
* `FOK` / `FAK`：立即吃单
  官方也是这么定义的；而且 market order 里的 `price` 是**最差成交价保护**，不是目标价。([Polymarket 文档][2])

### GTD

GTD 很适合事件前自动失效，但官方文档写了 **1 分钟安全阈值**：要想有效存活 N 秒，设置要用 `now + 60 + N`。([Polymarket 文档][2])

---

## 我建议的订单状态机

```text
Draft
  ↓
Planned
  ↓
RiskApproved
  ↓
Signed
  ↓
Submitted
  ↓
Acked
  ├─> Live
  │    ├─> PartiallyFilled
  │    │    ├─> Filled
  │    │    ├─> CancelPending
  │    │    └─> Expired
  │    ├─> CancelPending
  │    ├─> Cancelled
  │    └─> Expired
  ├─> Rejected
  └─> Unknown
```

### 为什么要有 `Unknown`

因为真实世界会有：

* 下单请求超时，但其实平台收到了
* user WS 掉线，ACK 丢了
* matching engine restart 返回 `425`
* 重启后本地状态和远端状态短暂不一致

Polymarket 官方明确写了 matching engine 每周二美东 7:00 重启，order 相关接口会回 `425`，应指数退避重试。([Polymarket 文档][6])

所以 `Unknown` 不是多余状态，而是恢复逻辑的支点：
**只要进入 Unknown，就禁止继续基于“我以为的持仓”扩仓，先 reconcile。**

---

## 数据库怎么建

我建议 Postgres，核心表如下。

### 1) `markets`

```sql
CREATE TABLE markets (
  market_id              TEXT PRIMARY KEY,
  condition_id           TEXT NOT NULL,
  event_id               TEXT,
  title                  TEXT NOT NULL,
  neg_risk               BOOLEAN NOT NULL DEFAULT FALSE,
  active                 BOOLEAN NOT NULL DEFAULT TRUE,
  closed                 BOOLEAN NOT NULL DEFAULT FALSE,
  end_time               TIMESTAMPTZ,
  created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### 2) `assets`

```sql
CREATE TABLE assets (
  asset_id               TEXT PRIMARY KEY,
  market_id              TEXT NOT NULL REFERENCES markets(market_id),
  outcome                TEXT NOT NULL, -- YES / NO / outcome label
  min_tick_size          NUMERIC(18,8) NOT NULL,
  winner                 BOOLEAN,
  created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

`neg_risk` 和 tick size 都要落表，因为官方接口和 WS 事件都直接依赖它们。([Polymarket 文档][7])

### 3) `balances`

```sql
CREATE TABLE balances (
  asset_key              TEXT PRIMARY KEY, -- USDC.e or asset_id
  free_amount            NUMERIC(38,18) NOT NULL,
  reserved_amount        NUMERIC(38,18) NOT NULL,
  total_amount           NUMERIC(38,18) NOT NULL,
  as_of                  TIMESTAMPTZ NOT NULL
);
```

### 4) `orders`

```sql
CREATE TABLE orders (
  internal_order_id      UUID PRIMARY KEY,
  venue_order_id         TEXT UNIQUE,
  strategy_name          TEXT NOT NULL,
  market_id              TEXT NOT NULL,
  asset_id               TEXT NOT NULL,
  side                   TEXT NOT NULL,          -- BUY / SELL
  order_type             TEXT NOT NULL,          -- GTC / GTD / FOK / FAK
  tif_expiration         TIMESTAMPTZ,
  price                  NUMERIC(18,8) NOT NULL,
  size                   NUMERIC(38,18) NOT NULL,
  size_filled            NUMERIC(38,18) NOT NULL DEFAULT 0,
  status                 TEXT NOT NULL,
  client_order_key       TEXT NOT NULL UNIQUE,
  batch_group_id         UUID,
  reject_reason          TEXT,
  created_at             TIMESTAMPTZ NOT NULL,
  updated_at             TIMESTAMPTZ NOT NULL
);
```

`client_order_key` 很重要，用来做幂等恢复。

### 5) `fills`

```sql
CREATE TABLE fills (
  trade_id               TEXT PRIMARY KEY,
  venue_order_id         TEXT NOT NULL,
  market_id              TEXT NOT NULL,
  asset_id               TEXT NOT NULL,
  side                   TEXT NOT NULL,
  price                  NUMERIC(18,8) NOT NULL,
  size                   NUMERIC(38,18) NOT NULL,
  fee_rate_bps           INTEGER,
  fee_paid               NUMERIC(38,18),
  tx_hash                TEXT,
  matched_at             TIMESTAMPTZ NOT NULL,
  inserted_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

user channel 的 trade event 里就有 `price / size / fee_rate_bps / transaction_hash` 等字段。([Polymarket 文档][4])

### 6) `positions`

```sql
CREATE TABLE positions (
  asset_id               TEXT PRIMARY KEY,
  position_size          NUMERIC(38,18) NOT NULL,
  avg_cost               NUMERIC(18,8) NOT NULL,
  realized_pnl           NUMERIC(38,18) NOT NULL DEFAULT 0,
  unrealized_pnl         NUMERIC(38,18) NOT NULL DEFAULT 0,
  updated_at             TIMESTAMPTZ NOT NULL
);
```

### 7) `ctf_operations`

```sql
CREATE TABLE ctf_operations (
  op_id                  UUID PRIMARY KEY,
  op_type                TEXT NOT NULL,          -- SPLIT / MERGE / REDEEM
  condition_id           TEXT NOT NULL,
  amount                 NUMERIC(38,18) NOT NULL,
  tx_hash                TEXT,
  status                 TEXT NOT NULL,
  requested_at           TIMESTAMPTZ NOT NULL,
  completed_at           TIMESTAMPTZ
);
```

### 8) `opportunities`

```sql
CREATE TABLE opportunities (
  opp_id                 UUID PRIMARY KEY,
  strategy_name          TEXT NOT NULL,
  market_id              TEXT NOT NULL,
  kind                   TEXT NOT NULL,          -- FULLSET_BUY_MERGE / FULLSET_SPLIT_SELL
  gross_edge             NUMERIC(18,8) NOT NULL,
  net_edge               NUMERIC(18,8) NOT NULL,
  size                   NUMERIC(38,18) NOT NULL,
  decision               TEXT NOT NULL,          -- APPROVED / REJECTED / EXECUTED / SKIPPED
  reason                 TEXT,
  created_at             TIMESTAMPTZ NOT NULL
);
```

### 9) `event_journal`

```sql
CREATE TABLE event_journal (
  seq_id                 BIGSERIAL PRIMARY KEY,
  stream                 TEXT NOT NULL,          -- market_ws / user_ws / internal_cmd / internal_evt
  event_type             TEXT NOT NULL,
  event_ts               TIMESTAMPTZ NOT NULL,
  payload                JSONB NOT NULL,
  inserted_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_event_journal_stream_ts ON event_journal(stream, event_ts);
```

这个表是整个系统的灵魂。
只要 journal 在，你后面就能：

* 重放 bug
* 重算策略
* 重建本地状态
* 做回测和 postmortem

---

## full-set 策略模块怎么写

### 输入

* 两个 asset 的 best bid / ask
* 当前 tick size
* fee params
* 账户可用 USDC.e / YES / NO
* 当前 event lifecycle
* 本地 open orders

### 输出

`Opportunity::FullSetBuyMerge` 或 `Opportunity::FullSetSplitSell`

### 核心公式

#### 买两边再 merge

```text
net_edge = 1
         - ask_yes
         - ask_no
         - taker_fee_yes
         - taker_fee_no
         - expected_slippage
         - ops_cost_buffer
```

#### split 再卖两边

```text
net_edge = bid_yes
         + bid_no
         - 1
         - taker_fee_yes
         - taker_fee_no
         - expected_slippage
         - ops_cost_buffer
```

Polymarket 官方 fee 不是常数，而是跟价格 `p` 相关；官方给了公式，且有效费率在 50% 概率附近最高。([Polymarket 文档][8])

所以你的 `pricing` crate 一定要单独写，不要把 fee 硬编码成 0。

---

## v1 最值得做的执行细节

### 1) 双腿执行

full-set 的下单一定要做成**同一 batch group**。
官方一次最多 15 单 batch，非常适合双腿同步提交。([Polymarket 文档][2])

### 2) FOK 还是 FAK

* **机会很薄**：优先 FOK
* **你允许部分成交后补腿**：用 FAK

因为官方定义里：

* FOK = 全成或全撤
* FAK = 能吃多少吃多少，剩下取消
  而 market order 的 `price` 只是最差保护价。([Polymarket 文档][2])

### 3) maker v1 不做

先不要在 v1 上 maker quote。
因为你还没把 inventory / cancel / sports / restart / heartbeat 跑稳之前，maker 只会把复杂度炸开。

---

## 哪些地方绝对不能省

### 不能省 1：user WS 心跳

官方 user channel 要客户端 heartbeat，文档写的是**每 10 秒发送一次**。([Polymarket 文档][4])

### 不能省 2：tick size 动态更新

market WS 会推 `tick_size_change`。不处理的话，合法价单都可能被你自己报成非法。([Polymarket 文档][3])

### 不能省 3：negRisk 标记

以后你做 neg-risk，官方元数据里有 `negRisk` 布尔字段；下单也要带 `negRisk: true`。([Polymarket 文档][7])

### 不能省 4：425 恢复逻辑

matching engine 每周二美东 7:00 重启，order 相关接口会回 `425`。([Polymarket 文档][6])

### 不能省 5：体育市场特殊规则

官方写明体育市场：

* 开赛时 outstanding limit orders 自动取消
* marketable orders 有 **3 秒** placement delay
* 开赛时间变化时要密切监控
  所以 v1 最好先**避开 sports**。([Polymarket 文档][2])

---

## 哪些地方 v1 可以故意偷懒

这些都可以先不做：

* Redis
* Kafka
* 微服务拆分
* maker rebate 结算精细优化
* 复杂多市场套利
* 自动资金桥接
* dashboard 大盘

### v1 最小可用栈

* Rust live engine
* Postgres
* Prometheus + Grafana
* Python notebooks
* 一套 Docker Compose

够了。

---

## 我建议的开发顺序

### 第 1 周

把“能连上 + 能重放”打通：

* 拉市场元数据
* 订阅 market WS
* 订阅 user WS
* 每 10 秒 heartbeat
* event_journal 落库
* market/order/trade 事件都能 replay

### 第 2 周

把“能准确认自己资产和挂单”打通：

* balances
* open orders
* positions
* reconcile
* Unknown 状态恢复

### 第 3 周

把 full-set 策略接上：

* best bid/ask 计算
* fee model
* net edge
* mock execution

### 第 4 周

真下最小仓位：

* batch 双腿
* FOK 优先
* merge / split
* 异常恢复
* PnL 归因

---

## 一个很实用的二进制划分

我建议一开始就有这 4 个可执行程序：

```text
cargo run -p app-live       # 正式 bot
cargo run -p app-replay     # 从 journal 重放
cargo run -p app-backfill   # 补元数据/补历史
cargo run -p strategy-fullset -- --paper
```

其中 `--paper` 模式非常重要：
先只“发现机会 + 模拟成交 + 不真实下单”，把 3~7 天的数据跑出来，验证：

* 机会频率
* 平均净 edge
* 可成交率
* 部分成交风险
* restart / reconnect 对收益的影响

---

## 最后给你一个“最推荐的落地组合”

**项目名**：`AxiomArb`
**v1 策略名**：`fullset`
**技术栈**：Rust + Postgres + Python
**工程形态**：单进程多任务，append-only journal
**第一阶段目标**：只做 full-set arb，不碰 sports，不做 maker
**第二阶段**：加入 neg-risk
**第三阶段**：再上 maker rebate / liquidity rewards 增强

[1]: https://docs.polymarket.com/api-reference/clients-sdks "Clients & SDKs - Polymarket Documentation"
[2]: https://docs.polymarket.com/trading/orders/create "Create Order - Polymarket Documentation"
[3]: https://docs.polymarket.com/api-reference/wss/market "Market Channel - Polymarket Documentation"
[4]: https://docs.polymarket.com/api-reference/wss/user "User Channel - Polymarket Documentation"
[5]: https://docs.polymarket.com/trading/ctf/overview?utm_source=chatgpt.com "Conditional Token Framework"
[6]: https://docs.polymarket.com/trading/matching-engine "Matching Engine Restarts - Polymarket Documentation"
[7]: https://docs.polymarket.com/advanced/neg-risk "Negative Risk Markets - Polymarket Documentation"
[8]: https://docs.polymarket.com/trading/fees "Fees - Polymarket Documentation"
