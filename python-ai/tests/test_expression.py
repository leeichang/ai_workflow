"""
條件表達式測試

安全性是這裡的重點。表達式來自流程設計器，設計者是租戶的使用者，
若能執行任意程式碼就等於租戶隔離失效——讀別的租戶資料、
讀環境變數裡的資料庫密碼、對內網發請求都做得到。
"""

from __future__ import annotations

import pytest

from dsl.expression import ExpressionError, evaluate


@pytest.fixture
def ctx():
    return {
        "quotation": {
            "discount_rate": 0.2,
            "amount": 500000,
            "status": "draft",
            "tags": ["urgent", "vip"],
            "customer": {"name": "台灣精密", "level": 3},
        },
        "form": {"approved": True, "note": None},
    }


class Test比較運算:
    def test_大於(self, ctx):
        assert evaluate("quotation.discount_rate > 0.15", ctx) is True
        assert evaluate("quotation.discount_rate > 0.25", ctx) is False

    def test_等於字串(self, ctx):
        assert evaluate("quotation.status == 'draft'", ctx) is True
        assert evaluate("quotation.status == 'sent'", ctx) is False

    def test_巢狀路徑(self, ctx):
        assert evaluate("quotation.customer.level >= 3", ctx) is True

    def test_in_運算(self, ctx):
        assert evaluate("'urgent' in quotation.tags", ctx) is True
        assert evaluate("'normal' in quotation.tags", ctx) is False

    def test_連鎖比較(self, ctx):
        assert evaluate("0 < quotation.discount_rate < 1", ctx) is True


class Test布林運算:
    def test_and(self, ctx):
        assert evaluate("quotation.amount > 100 and form.approved", ctx) is True
        assert evaluate("quotation.amount > 999999 and form.approved", ctx) is False

    def test_or(self, ctx):
        assert evaluate("quotation.amount > 999999 or form.approved", ctx) is True

    def test_not(self, ctx):
        assert evaluate("not form.approved", ctx) is False


class Test缺漏資料:
    def test_路徑不存在視為_none_而非拋錯(self, ctx):
        # 設計流程時欄位可能還沒建。讓整個流程掛掉不如讓條件為假。
        assert evaluate("quotation.not_exist > 5", ctx) is False

    def test_根層不存在(self, ctx):
        assert evaluate("nothing.at.all == 1", ctx) is False

    def test_none_參與大小比較為假(self, ctx):
        assert evaluate("form.note > 5", ctx) is False
        assert evaluate("form.note < 5", ctx) is False

    def test_none_可以等於比較(self, ctx):
        # 等於比較有明確語意，不該跟大小比較一樣一律為假
        assert evaluate("form.note == None", ctx) is True

    def test_型別不可比較時為假(self, ctx):
        assert evaluate("quotation.status > 5", ctx) is False


class Test安全性:
    """以下每一項若通過求值，就是一個遠端執行漏洞"""

    @pytest.mark.parametrize(
        "expr",
        [
            "__import__('os').system('id')",
            "open('/etc/passwd').read()",
            "quotation.__class__.__bases__",
            "(1).__class__.__mro__[1].__subclasses__()",
            "eval('1+1')",
            "exec('x=1')",
            "[x for x in range(10)]",
            "lambda: 1",
            "quotation.update({'amount': 0})",
        ],
    )
    def test_危險表達式一律拒絕(self, ctx, expr):
        with pytest.raises(ExpressionError):
            evaluate(expr, ctx)

    def test_函式呼叫被拒絕(self, ctx):
        # ast.Call 不在白名單內。允許任何呼叫都等於開後門。
        with pytest.raises(ExpressionError, match="不支援的語法"):
            evaluate("len(quotation.tags) > 1", ctx)

    def test_算術運算被拒絕(self, ctx):
        # 刻意不支援。需要算術時應在表單的 computed 欄位算好，
        # 條件式只做判斷。支援算術會讓白名單持續膨脹。
        with pytest.raises(ExpressionError):
            evaluate("quotation.amount * 2 > 100", ctx)

    def test_雙底線屬性在解析階段就被拒絕(self, ctx):
        # 不能只靠「資料剛好都是 dict，取不到 __class__」。
        # 只要有一個值是物件，getattr 就能沿 __class__ 挖到
        # __subclasses__，那是經典的沙箱逃逸路徑。
        with pytest.raises(ExpressionError, match="內部屬性"):
            evaluate("quotation.__class__ == None", ctx)

    def test_雙底線名稱也被拒絕(self, ctx):
        with pytest.raises(ExpressionError, match="內部名稱"):
            evaluate("__builtins__ == None", ctx)


class Test輸入驗證:
    def test_空字串拋錯(self, ctx):
        with pytest.raises(ExpressionError, match="不可為空"):
            evaluate("", ctx)

    def test_只有空白拋錯(self, ctx):
        with pytest.raises(ExpressionError):
            evaluate("   ", ctx)

    def test_語法錯誤訊息可讀(self, ctx):
        with pytest.raises(ExpressionError, match="語法錯誤"):
            evaluate("quotation.amount >", ctx)


class Test決定性:
    def test_同樣輸入必得同樣輸出(self, ctx):
        # Workflow 重放時若結果不同，Temporal 會判定非決定性而失敗
        expr = "quotation.discount_rate > 0.15 and quotation.status == 'draft'"
        results = {evaluate(expr, ctx) for _ in range(50)}
        assert results == {True}
