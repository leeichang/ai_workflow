"""
條件表達式求值

DSL 的 condition 節點帶表達式，例如：
    quotation.discount_rate > 0.15
    form.amount > 100000 and quotation.status == 'draft'

**不可使用 eval()。** 表達式來自流程設計器，設計者是租戶的使用者，
不是平台管理員。eval 會讓任何能編輯流程的人在 Worker 上執行
任意程式碼——讀取其他租戶的資料、發起網路連線、讀環境變數裡的
資料庫密碼。這是租戶隔離的破口，不是效能或風格問題。

改以 ast 模組解析後走白名單直譯：只允許比較、布林運算、
字面值與資料路徑，其餘節點型別一律拒絕。

求值必須是純函數，Workflow 重放時同樣的輸入要得到同樣的結果。
"""

from __future__ import annotations

import ast
from typing import Any

# 允許的 AST 節點。白名單而非黑名單：
# 新版 Python 增加節點型別時，預設是拒絕而非放行。
_ALLOWED_NODES = (
    ast.Expression,
    ast.BoolOp,
    ast.UnaryOp,
    ast.Compare,
    ast.Name,
    ast.Attribute,
    ast.Constant,
    ast.List,
    ast.Tuple,
    ast.And,
    ast.Or,
    ast.Not,
    ast.Eq,
    ast.NotEq,
    ast.Lt,
    ast.LtE,
    ast.Gt,
    ast.GtE,
    ast.In,
    ast.NotIn,
    ast.Load,
)


class ExpressionError(ValueError):
    """表達式無效。訊息會回到流程設計器顯示給使用者。"""


def evaluate(expression: str, context: dict[str, Any]) -> bool:
    """
    求值並轉為布林

    context 為資料根，例如 {"quotation": {...}, "form": {...}}。
    路徑不存在時視為 None 而非拋錯：設計流程時欄位可能還沒建，
    讓整個流程掛掉不如讓條件為假。
    """
    if not expression or not expression.strip():
        raise ExpressionError("條件表達式不可為空")

    try:
        tree = ast.parse(expression, mode="eval")
    except SyntaxError as e:
        raise ExpressionError(f"語法錯誤：{e.msg}") from e

    _check_allowed(tree)
    return bool(_eval_node(tree.body, context))


def _check_allowed(tree: ast.AST) -> None:
    for node in ast.walk(tree):
        if not isinstance(node, _ALLOWED_NODES):
            raise ExpressionError(
                f"不支援的語法：{type(node).__name__}。"
                "只允許比較、and/or/not、數值與欄位路徑"
            )

        # 雙底線屬性一律拒絕。單靠節點白名單擋不住：
        #   quotation.__class__.__bases__
        # 是合法的 Attribute 鏈。目前資料都是 dict，走 .get() 取不到，
        # 但只要有一個值是物件，getattr 就會沿 __class__ 一路挖到
        # __subclasses__，那是經典的沙箱逃逸路徑。
        # 不依賴「資料剛好都是 dict」這個前提。
        if isinstance(node, ast.Attribute) and node.attr.startswith("__"):
            raise ExpressionError(f"不允許存取內部屬性：{node.attr}")
        if isinstance(node, ast.Name) and node.id.startswith("__"):
            raise ExpressionError(f"不允許存取內部名稱：{node.id}")


def _eval_node(node: ast.AST, ctx: dict[str, Any]) -> Any:
    if isinstance(node, ast.Constant):
        return node.value

    if isinstance(node, ast.Name):
        return ctx.get(node.id)

    if isinstance(node, ast.Attribute):
        base = _eval_node(node.value, ctx)
        if isinstance(base, dict):
            return base.get(node.attr)
        return getattr(base, node.attr, None)

    if isinstance(node, (ast.List, ast.Tuple)):
        return [_eval_node(e, ctx) for e in node.elts]

    if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.Not):
        return not _eval_node(node.operand, ctx)

    if isinstance(node, ast.BoolOp):
        values = node.values
        if isinstance(node.op, ast.And):
            # 短路求值。and 的右側可能依賴左側成立，
            # 例如 items != None and items.count > 0
            for v in values:
                if not _eval_node(v, ctx):
                    return False
            return True
        for v in values:
            if _eval_node(v, ctx):
                return True
        return False

    if isinstance(node, ast.Compare):
        left = _eval_node(node.left, ctx)
        for op, comparator in zip(node.ops, node.comparators):
            right = _eval_node(comparator, ctx)
            if not _compare(left, op, right):
                return False
            left = right
        return True

    raise ExpressionError(f"無法求值的節點：{type(node).__name__}")


def _compare(left: Any, op: ast.cmpop, right: Any) -> bool:
    if isinstance(op, ast.Eq):
        return left == right
    if isinstance(op, ast.NotEq):
        return left != right
    if isinstance(op, ast.In):
        return right is not None and left in right
    if isinstance(op, ast.NotIn):
        return right is None or left not in right

    # 大小比較遇到 None 一律為假，不拋錯。
    # 欄位還沒填時 quotation.amount > 100 應該是「不成立」，
    # 而不是讓整個流程因 TypeError 失敗。
    if left is None or right is None:
        return False

    try:
        if isinstance(op, ast.Lt):
            return left < right
        if isinstance(op, ast.LtE):
            return left <= right
        if isinstance(op, ast.Gt):
            return left > right
        if isinstance(op, ast.GtE):
            return left >= right
    except TypeError:
        # 型別不可比較（字串比數字）同樣視為不成立
        return False

    raise ExpressionError(f"不支援的比較運算：{type(op).__name__}")
