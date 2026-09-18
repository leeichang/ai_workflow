# Workflow Data Connection  
  
可以。這一塊我會刻意和你前面設計的 **Enterprise Integration Gateway（EIG）** 分開。  
我建議你的 **Workflow Data Connection（WDC）** 不要被設計成「另一套 Connector」，而是：  
**Workflow 在執行期間取得、轉換、寫回資料的統一資料存取抽象層。**  
這樣 EIG 負責「企業資料進入平台」，WDC 負責「Workflow 怎麼使用資料」。  
   
⸻  
   
## 一、先定義兩者的邊界  
我建議整個平台是：  
```
┌──────────────────────────────────────────────┐
│              Workflow Platform               │
│                                              │
│  Form      Workflow       AI Agent           │
│   │           │              │               │
│   └───────────┼──────────────┘               │
│               ↓                              │
│       Workflow Data Connection               │
│               │                              │
│      ┌────────┼─────────┐                    │
│      ↓        ↓         ↓                    │
│    Cache    External   Dataset               │
│              API                              │
└──────────────┼───────────────────────────────┘
               ↓
     Enterprise Integration Gateway
               ↓
       ┌───────┼─────────┐
       ↓       ↓         ↓
      SAP     Odoo      MES

```
**EIG 解決：**  
「企業資料怎麼進入我的平台？」  
**WDC 解決：**  
「Workflow 怎麼使用資料？」  
這個界線非常重要。  
   
⸻  
   
## 二、WDC 的核心概念  
我建議一個 Workflow 不要直接寫：  
```
GET https://sap.xxx.com/api/...

```
也不要：  
```
SELECT * FROM employee

```
而是：  
```
Workflow
   ↓
Data Connection
   ↓
Operation
   ↓
Input Mapping
   ↓
Execution
   ↓
Output Mapping
   ↓
Workflow Dataset

```
例如：  
```
取得申請人的部門主管

```
Workflow 只知道：  
```
Connection:
Employee

Operation:
get_manager

Input:
employee_id

Output:
manager

```
至於後面：  
```
PostgreSQL Cache
SAP RFC
Odoo API
REST API
SQL

```
Workflow 完全不知道。  
   
⸻  
   
## 三、WDC 我會設計成 5 種 Data Source  
```
Workflow Data Connection
│
├── 1. Internal Dataset
├── 2. Integration Cache
├── 3. External API
├── 4. Database Query
└── 5. File / Document

```
**1. Internal Dataset**  
平台自己產生的資料。  
例如：  
```
purchase_request
approval_task
customer_case
workflow_instance

```
   
⸻  
   
**2. Integration Cache**  
來自你的 EIG。  
例如：  
```
employee
department
project
role
customer
supplier

```
這應該是 Workflow 最常使用的資料。  
   
⸻  
   
**3. External API**  
例如：  
```
REST
SOAP
GraphQL
Webhook

```
   
⸻  
   
**4. Database**  
例如：  
```
PostgreSQL
SQL Server
Oracle
MySQL

```
但我會非常小心。  
**不要讓 Workflow 使用者直接輸入任意 SQL。**  
應該做：  
```
Data Query Definition

```
例如：  
```
Employee Search

```
參數：  
```
department_id
status

```
SQL 是 Connection Owner 管理的。  
   
⸻  
   
**5. File / Document**  
例如：  
```
CSV
Excel
JSON
XML
PDF

```
但 PDF 我會讓：  
```
Document Connection
       ↓
AI Extraction

```
而不是讓 WDC 自己理解 PDF。  
   
⸻  
   
## 四、最重要的設計：Connection ≠ Operation  
這是我非常建議你採用的模型。  
不要：  
```
Connection = REST API

```
而是：  
```
Connection
   │
   ├── Operation A
   ├── Operation B
   └── Operation C

```
例如：  
```
Employee Service
│
├── get_employee
├── search_employee
├── get_manager
└── get_department_members

```
   
⸻  
   
## 五、完整模型  
可以定義：  
```
Data Connection
      │
      └── Data Operation
               │
               ├── Input Schema
               ├── Request
               ├── Output Schema
               └── Error Schema

```
例如：  
```
Employee Connection

Operation:
get_manager

```
Input：  
```
{
  "employee_id": "E001"
}

```
Output：  
```
{
  "employee_id": "E0005",
  "name": "王小明",
  "email": "xxx@example.com"
}

```
   
⸻  
   
## 六、Connection 的完整設定  
我會設計成：  
```
Connection
├── General
├── Authentication
├── Endpoint
├── Request
├── Response
├── Schema
├── Mapping
├── Error
├── Security
├── Cache
└── Version

```
   
⸻  
   
## 七、General  
```
Name
Code
Description
Type
Environment
Status
Owner
Tags

```
例如：  
```
Name:
ERP Employee API

Code:
erp_employee

Type:
REST

Environment:
Production

```
   
⸻  
   
## 八、Authentication  
不要把 Credential 直接放在 Connection JSON。  
應該：  
```
Connection
   ↓
Credential Reference
   ↓
Secret Vault

```
支援：  
```
None
API Key
Basic Auth
Bearer Token
OAuth2
JWT
mTLS
SSH
Database Credential
SAP Credential

```
   
⸻  
   
## 九、Request Definition  
例如 REST：  
```
{
  "method": "GET",
  "url": "/api/employees/{employee_id}",
  "headers": {
    "Accept": "application/json"
  },
  "query": {},
  "path": {
    "employee_id": "$input.employee_id"
  }
}

```
這裡：  
```
$input.employee_id

```
就是 Workflow Input。  
   
⸻  
   
## 十、Response Parser  
這是你之前「解析 return」的概念。  
REST 回傳：  
```
{
  "success": true,
  "data": {
    "id": "E001",
    "name": "John",
    "department": {
      "id": "D001"
    }
  }
}

```
WDC：  
```
Response Parser
       ↓
JSONPath / JMESPath
       ↓
Output

```
例如：  
```
$.data

```
   
⸻  
   
## 十一、Output Schema  
我強烈建議：  
**每一個 Operation 必須有 Output Schema。**  
例如：  
```
{
  "type": "object",
  "properties": {
    "employee_id": {
      "type": "string"
    },
    "name": {
      "type": "string"
    },
    "department_id": {
      "type": "string"
    }
  }
}

```
這會帶來非常大的好處。  
Workflow Designer 可以做到：  
```
API Node
 ↓
Output
 ├── employee_id
 ├── name
 └── department_id

```
然後使用者可以直接拖：  
```
department_id
        ↓
Condition

```
   
⸻  
   
## 十二、Input / Output Mapping  
這是 WDC 最核心的功能之一。  
例如：  
Workflow Dataset：  
```
{
  "requester": {
    "employee_id": "E001"
  }
}

```
Operation：  
```
get_manager

```
需要：  
```
employee_id

```
Mapping：  
```
requester.employee_id
        ↓
input.employee_id

```
回傳：  
```
output.employee_id
        ↓
approval.approver_id

```
   
⸻  
   
## 十三、Mapping Designer  
我甚至會做一個視覺化 Mapping：  
```
Workflow Data              Operation Input

requester.employee_id ─────────→ employee_id

request.amount ────────────────→ amount

```
Output：  
```
Operation Output             Workflow Data

employee_id ─────────────────→ approver.id

name ────────────────────────→ approver.name

email ───────────────────────→ approver.email

```
這是 SME 很容易理解的 UX。  
   
⸻  
   
## 十四、Data Transformation  
Mapping 不應該只有一對一。  
例如：  
```
amount
 ↓
number()
 ↓
round(2)
 ↓
input.amount

```
應該支援基本 Expression：  
```
trim()
uppercase()
lowercase()
number()
string()
date()
date_add()
concat()
split()
replace()
round()
coalesce()

```
例如：  
```
concat(first_name, " ", last_name)

```
   
⸻  
   
## 十五、不要讓使用者直接寫任意程式  
例如不要讓：  
```
eval(...)

```
這種東西直接執行。  
我建議：  
```
Expression DSL

```
例如：  
```
amount > 100000

```
或：  
```
employee.department_id == request.department_id

```
未來可以做：  
```
CEL
JsonLogic
JMESPath

```
其中我會優先考慮 **CEL** 類型的安全 expression language。  
   
⸻  
   
## 十六、WDC 支援 Read / Write  
Operation 不應只有 GET。  
應該：  
```
Operation Type

QUERY
CREATE
UPDATE
DELETE
EXECUTE

```
例如：  
```
Employee
├── get
├── search
└── get_manager

Purchase
├── get
├── create
├── update
└── submit

```
   
⸻  
   
## 十七、Workflow 裡的 Data Node  
這樣 Workflow Designer 就可以出現：  
```
Trigger
   ↓
Get Data
   ↓
Condition
   ↓
Human Approval
   ↓
Update Data
   ↓
Notification

```
例如：  
```
Trigger: Purchase Request Created

        ↓

Get Department Manager

        ↓

amount > 100,000 ?

       YES
        ↓

Get Finance Manager

        ↓

Parallel Approval

        ↓

Update ERP Purchase Request

        ↓

Notification

```
   
⸻  
   
## 十八、Data Connection 不應該只服務 Workflow  
這點非常重要。  
同一個 WDC 應該可以被：  
```
Workflow
Form
AI Agent
Report
Dashboard

```
共同使用。  
例如：  
```
Employee Connection
       │
 ┌─────┼─────────────┐
 ↓     ↓             ↓
Form  Workflow      AI

```
Form：  
```
選擇員工

```
Workflow：  
```
找主管

```
AI：  
```
查詢員工資料

```
全部使用同一個 Connection。  
   
⸻  
   
## 十九、這會讓你的 Form Engine 變得非常強  
例如 Form 有：  
```
申請人

```
Component：  
```
Reference

```
設定：  
```
Data Source:
Employee Connection

Operation:
search_employee

```
輸入：  
```
keyword

```
回傳：  
```
employee_id
name
department

```
使用者選：  
```
王小明

```
Form 存：  
```
{
  "employee_id": "E001"
}

```
而不是把整個 Employee Object 塞進表單。  
   
⸻  
   
## 二十、AI Agent 也使用 WDC  
這是我認為你整個平台很有潛力的地方。  
AI Agent 不應該直接知道：  
```
SAP API
Odoo API
Oracle

```
它只能看到：  
```
Tools
│
├── get_employee
├── search_customer
├── get_inventory
├── create_purchase_request
└── get_project

```
也就是：  
```
AI Agent
     ↓
Tool Registry
     ↓
Workflow Data Connection
     ↓
Enterprise System

```
這就可以做到：  
**AI Agent 不直接接觸企業系統，而是只能呼叫被授權的 Data Operations。**  
安全性會好非常多。  
   
⸻  
   
## 二十一、我要特別增加 Tool Permission  
每一個 Operation 都要有：  
```
permission

```
例如：  
```
employee.search
employee.get

purchase.read
purchase.create
purchase.update

purchase.approve

```
AI Agent：  
```
Inventory Agent

```
只能：  
```
inventory.read

```
不能：  
```
purchase.create

```
   
⸻  
   
## 二十二、Cache Policy  
WDC 可以使用 EIG Cache。  
例如：  
```
Connection:
Employee

Cache:
Integration Cache

TTL:
15 minutes

```
Workflow：  
```
Get Employee
 ↓
Cache
 ↓
return

```
另一個：  
```
Connection:
Inventory

Cache:
None

Mode:
Real-time

```
就直接：  
```
ERP API

```
   
⸻  
   
## 二十三、我會提供三種 Data Access Mode  
**CACHE**  
```
Workflow
 ↓
Cache

```
最快。  
   
⸻  
   
**REALTIME**  
```
Workflow
 ↓
External System

```
資料最新。  
   
⸻  
   
**CACHE_THEN_REFRESH**  
```
Workflow
 ↓
Cache
 ↓
過期？
 ├── NO → return
 └── YES → refresh

```
   
⸻  
   
## 二十四、Offline / Failure Policy  
企業 API 一定會掛。  
所以 Connection 要有：  
```
Failure Policy

```
例如：  
```
FAIL
RETRY
USE_CACHE
SKIP
MANUAL

```
例如：  
```
取得員工主管

API failure
 ↓
Cache available?
 ↓
YES
 ↓
Use cache
 ↓
Mark data as stale

```
但財務付款：  
```
Payment API failure
 ↓
DO NOT use cache
 ↓
Retry
 ↓
Human Task

```
   
⸻  
   
## 二十五、Retry Policy  
```
max_retry = 3

backoff:
5s
30s
5m

```
並支援：  
```
Timeout
Circuit Breaker
Rate Limit
Concurrency Limit

```
這些不要讓每個 Workflow 自己設定。  
應該：  
```
Connection Policy

```
統一管理。  
   
⸻  
   
## 二十六、Data Contract  
我會讓 WDC 與 Data Contract 緊密合作。  
完整流程：  
```
Connection
   ↓
Operation
   ↓
Input Contract
   ↓
Request
   ↓
External System
   ↓
Response
   ↓
Parser
   ↓
Output Contract
   ↓
Workflow Dataset

```
例如：  
```
Employee.get_manager

Input Contract
{
  employee_id: string
}

Output Contract
{
  employee_id: string
  name: string
  email: string
}

```
   
⸻  
   
## 二十七、Schema Registry  
如果你要做長期產品，我建議一定要有：  
```
Schema Registry

```
例如：  
```
employee.v1
employee.v2
employee.v3

```
Workflow：  
```
Workflow V1
 ↓
employee.v1

```
新流程：  
```
Workflow V2
 ↓
employee.v2

```
不要因為 ERP API 改欄位就把舊 Workflow 弄壞。  
   
⸻  
   
## 二十八、Connection Version  
同樣需要：  
```
Connection v1
Connection v2

```
例如：  
```
API v1:
employee_no

API v2:
employee_code

```
你的 Mapping 可以維持：  
```
Canonical:
employee.employee_no

```
而 Connection v2：  
```
employee_code
 ↓
employee.employee_no

```
Workflow 不需要修改。  
   
⸻  
   
## 二十九、完整 Database 初版  
我會先設計：  
```
integration_connection
integration_connection_version

integration_credential_ref

integration_operation
integration_operation_version

integration_input_schema
integration_output_schema

integration_mapping

integration_dataset

integration_cache_policy

integration_execution

integration_execution_log

integration_error

integration_schema
integration_schema_version

```
Workflow：  
```
workflow_definition
workflow_version
workflow_instance
workflow_execution
workflow_task
workflow_dataset

```
兩邊不要硬塞在同一張表。  
   
⸻  
   
## 三十、Integration Execution Log  
每次 WDC 呼叫都要記錄：  
```
execution_id
connection_id
operation_id
workflow_instance_id
workflow_node_id

started_at
finished_at

status

request_metadata
response_metadata

input_hash
output_hash

error_code
error_message

```
但是：  
**不要把 API Password / Token / PII 全部寫進 Log。**  
   
⸻  
   
## 三十一、Workflow Data Context  
Workflow Runtime 裡，我建議有一個：  
```
Workflow Data Context

```
例如：  
```
{
  "workflow": {},
  "trigger": {},
  "form": {},
  "variables": {},
  "datasets": {},
  "tasks": {},
  "results": {}
}

```
例如：  
```
{
  "form": {
    "amount": 350000,
    "requester_id": "E001"
  },

  "datasets": {
    "requester": {},
    "department": {},
    "manager": {}
  }
}

```
Data Connection 執行結果放：  
```
datasets.manager

```
Condition 可以：  
```
datasets.manager.status == "active"

```
   
⸻  
   
## 三十二、這裡有一個非常重要的設計  
**不要把所有資料都塞進 Workflow Context。**  
例如 ERP 有：  
```
10 萬筆庫存

```
不要：  
```
Workflow Context = 10萬筆

```
應該：  
```
Dataset Reference

```
例如：  
```
{
  "dataset": "inventory",
  "query": {
    "product_id": "P001"
  }
}

```
必要時才 materialize。  
   
⸻  
   
## 三十三、因此我會把 Dataset 分成三種  
```
INLINE
REFERENCE
STREAM

```
**INLINE**  
小資料：  
```
{
  "employee_id": "E001"
}

```
**REFERENCE**  
大型資料：  
```
dataset_id = DS-10001

```
**STREAM**  
大量資料：  
```
10萬筆
100萬筆

```
交給 Worker 處理。  
這會讓你的 Workflow Runtime 更容易控制 Memory。  
   
⸻  
   
## 三十四、WDC Designer 我會設計成這樣  
```
┌─────────────────────────────────────────────┐
│ Data Connection Designer                    │
├─────────────────────────────────────────────┤
│ General                                     │
│                                             │
│ Name: Employee Service                      │
│ Type: REST                                  │
│                                             │
├─────────────────────────────────────────────┤
│ Authentication                              │
│                                             │
│ Credential: HR-API                          │
│                                             │
├─────────────────────────────────────────────┤
│ Operations                                  │
│                                             │
│ ● search_employee                           │
│ ● get_employee                              │
│ ● get_manager                               │
│                                             │
├─────────────────────────────────────────────┤
│ Input Schema                                │
│                                             │
│ employee_id : string                        │
│                                             │
├─────────────────────────────────────────────┤
│ Request                                     │
│                                             │
│ GET /employees/{employee_id}                │
│                                             │
├─────────────────────────────────────────────┤
│ Response                                    │
│                                             │
│ JSONPath: $.data                            │
│                                             │
├─────────────────────────────────────────────┤
│ Output Schema                               │
│                                             │
│ employee_id : string                        │
│ name        : string                        │
│ email       : string                        │
│                                             │
├─────────────────────────────────────────────┤
│ Test                                        │
│                                             │
│ [ Test Connection ]                         │
│ [ Test Operation ]                          │
└─────────────────────────────────────────────┘

```
   
⸻  
   
## 三十五、再加一個 AI Mapping  
這會非常符合你的產品定位。  
使用者拿到：  
```
{
  "PERNR": "10001",
  "ENAME": "王小明",
  "ORGEH": "D001",
  "MAIL": "xxx"
}

```
你的 AI：  
```
請問哪些欄位對應 Employee Schema？

```
AI 推測：  
```
PERNR → employee_no
ENAME → name
ORGEH → department_id
MAIL → email

```
然後：  
「AI 建議，請確認。」  
使用者按：  
```
✓ Accept

```
這會大幅降低 Integration 建置成本。  
   
⸻  
   
## 三十六、再進一步：Connection Template  
你前面說：  
不需要知道客戶用哪一個 ERP。  
我同意。  
但你仍然可以建立：  
```
Connection Template

```
例如：  
```
REST API
SQL Database
SAP RFC
CSV Import
Excel Import
SFTP
Webhook

```
甚至：  
```
SAP Employee Template
Odoo Employee Template

```
但它們只是：  
**Template**  
不是：  
Core Architecture。  
這樣你可以同時得到：  
**抽象架構 + 快速導入。**  
   
⸻  
   
## 三十七、我會把整個 Integration Layer 定義成四層  
這是我最推薦你的最終架構：  
```
                    Workflow / AI / Form
                            │
                            ↓
                ┌───────────────────────┐
                │ Workflow Data         │
                │ Connection            │
                └───────────┬───────────┘
                            │
                    Data Operation
                            │
                            ↓
                ┌───────────────────────┐
                │ Enterprise Integration│
                │ Gateway               │
                └───────────┬───────────┘
                            │
                    Adapter / Connector
                            │
          ┌─────────────────┼─────────────────┐
          ↓                 ↓                 ↓
        REST               SQL               RFC
          ↓                 ↓                 ↓
         API               DB                SAP

```
再加：  
```
EIG
 │
 ├── Raw Data
 ├── Mapping
 ├── Canonical Data
 ├── Cache
 ├── Sync
 └── Data Quality

```
   
⸻  
   
## 三十八、最終我會讓 Workflow Designer 只看到這些  
使用者拖一個：  
```
[Get Data]

```
Property：  
```
Connection:
Employee

Operation:
Get Manager

Input:
employee_id ← ${form.requester_id}

Output:
manager → datasets.manager

```
就結束。  
使用者**完全不需要知道**：  
```
SAP RFC
Odoo API
PostgreSQL
REST
Excel

```
   
⸻  
   
## 三十九、而且你可以讓同一個 Workflow 換企業而不修改流程  
這是你的架構真正漂亮的地方。  
**Company A**  
```
Employee
 ↓
SAP

```
**Company B**  
```
Employee
 ↓
Odoo

```
**Company C**  
```
Employee
 ↓
HR API

```
**Company D**  
```
Employee
 ↓
Excel

```
Workflow 永遠：  
```
Get Employee
      ↓
Get Manager
      ↓
Approval

```
完全相同。  
   
⸻  
   
## 四十、我會把你的產品核心抽象成這張圖  
```
                    BUSINESS WORKFLOW
                           │
                           ↓
                    ┌─────────────┐
                    │ Data Context│
                    └──────┬──────┘
                           │
                    ┌──────┴──────┐
                    ↓             ↓
                  READ           WRITE
                    │             │
                    └──────┬──────┘
                           ↓
                 Workflow Data Connection
                           │
                     Data Operation
                           │
                ┌──────────┴──────────┐
                ↓                     ↓
         Integration Gateway      Internal Dataset
                │
       ┌────────┼────────┐
       ↓        ↓        ↓
      API      SQL      File
       │        │        │
      SAP     Odoo      Excel

```
**我認為這個架構有一個很重要的結果：**  
你的產品最後不是：  
**Workflow Engine + 一堆 Connector**  
而是：  
**Workflow Engine + Data Abstraction Layer + Enterprise Integration Gateway**  
三者形成完整閉環。  
而 **WDC 是 Workflow 世界和 Enterprise Data 世界之間的「抽象 API」**。  
這也讓你前面談的 **Form、Parallel Approval、Email/Mobile Sign、AI Agent、Data Contract、Enterprise Integration Gateway** 全部可以共用同一套資料抽象，而不是各自發展一套 Integration 機制。  
如果你要進一步落地，我下一步會直接把這個 WDC 定義成一套可實作的 ****Workflow Data Connection Specification v1.0****，包含 **PostgreSQL ERD、JSON Schema、Connection/Operation DSL、REST API、Vue3 Designer 畫面、Mapping Engine、Runtime 執行流程，以及 Rust/Axum 的模組結構**。這會比繼續增加功能清單更接近真正可以開始寫 MVP 的程度。  
  
