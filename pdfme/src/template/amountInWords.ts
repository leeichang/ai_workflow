/**
 * 金額中文大寫
 *
 * 設計稿的「報價總計中文大寫」欄位。台灣單據的慣例寫法，
 * 用途是防止金額被竄改——阿拉伯數字容易改，「壹」不容易改成「貳」。
 *
 * 因為是防竄改用途，實作必須精確。四位一節（萬、億）是中文的
 * 分節方式，與英文三位一節（thousand、million）不同。
 */

const DIGITS = ['零', '壹', '貳', '參', '肆', '伍', '陸', '柒', '捌', '玖'];
const UNITS = ['', '拾', '佰', '仟'];
/** 節單位。中文四位一節，不是三位。 */
const SECTIONS = ['', '萬', '億', '兆'];

/** 上限：兆以上沒有對應的節單位，超過就不保證正確 */
const MAX = 9999_9999_9999_9999;

/**
 * 轉換整數金額為中文大寫
 *
 * 只處理整數。報價金額若有小數，呼叫端應先決定進位規則——
 * 這裡替它四捨五入會讓大寫與阿拉伯數字對不起來，
 * 而那正是這個欄位要防範的事。
 */
export function amountToChinese(amount: number): string {
  if (!Number.isFinite(amount)) return '';
  if (!Number.isInteger(amount)) {
    throw new Error(`金額必須為整數，收到 ${amount}。小數的進位規則應由呼叫端決定`);
  }
  if (amount < 0) return `負${amountToChinese(-amount)}`;
  if (amount === 0) return '零元整';
  if (amount > MAX) {
    throw new Error(`金額超過可表示範圍（${MAX}）`);
  }

  const sections: string[] = [];
  let remaining = amount;
  let sectionIndex = 0;

  // 由低位往高位逐節處理。sections[0] 永遠是目前最低的那一節。
  while (remaining > 0) {
    const section = remaining % 10000;
    const text = convertSection(section);

    if (text) {
      sections.unshift(text + SECTIONS[sectionIndex]);
    } else if (sections.length > 0 && !sections[0].startsWith(DIGITS[0])) {
      // 整節為零時補零，例如 100000008 是「壹億零捌元」而非「壹億捌元」，
      // 後者會被讀成一億八千萬。
      sections.unshift(DIGITS[0]);
    }

    remaining = Math.floor(remaining / 10000);
    sectionIndex += 1;

    // 本節不足四位且上方還有更高位時，要在本節之前補零。
    // 100001 的個位節值為 1（只有個位，缺了三位），
    // 因此是「壹拾萬零壹元」而非「壹拾萬壹元」——後者會讀成十萬一千。
    //
    // 判斷時機在推進之後：此時 remaining 代表更高位是否還有值。
    if (text && section < 1000 && remaining > 0 && !sections[0].startsWith(DIGITS[0])) {
      sections.unshift(DIGITS[0]);
    }
  }

  return `${sections.join('')}元整`;
}

/** 轉換一節（四位以內） */
function convertSection(value: number): string {
  if (value === 0) return '';

  let result = '';
  let zeroPending = false;

  for (let pos = 3; pos >= 0; pos--) {
    const digit = Math.floor(value / 10 ** pos) % 10;

    if (digit === 0) {
      // 尾端的零不寫，中間的零合併成一個
      if (result) zeroPending = true;
      continue;
    }

    if (zeroPending) {
      result += DIGITS[0];
      zeroPending = false;
    }
    result += DIGITS[digit] + UNITS[pos];
  }

  return result;
}

/** 供顯示用的完整字串，例如「新台幣 柒拾參萬捌仟壹佰伍拾元整」 */
export function formatAmountInWords(amount: number, currency = '新台幣'): string {
  return `${currency} ${amountToChinese(amount)}`;
}
