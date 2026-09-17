/**
 * 金額中文大寫測試
 *
 * 這個欄位的用途是防竄改，寫錯就失去意義。
 * 零的處理是最容易出錯的地方：中文四位一節，
 * 「壹億零捌元」與「壹億捌元」差了一億七千九百九十九萬九千九百九十二元。
 */

import { describe, expect, it } from 'vitest';
import { amountToChinese, formatAmountInWords } from '../src/template/amountInWords.js';

describe('基本數值', () => {
  it.each([
    [0, '零元整'],
    [1, '壹元整'],
    [9, '玖元整'],
    [10, '壹拾元整'],
    [11, '壹拾壹元整'],
    [100, '壹佰元整'],
    [1000, '壹仟元整'],
    [1234, '壹仟貳佰參拾肆元整'],
  ])('%i → %s', (amount, expected) => {
    expect(amountToChinese(amount)).toBe(expected);
  });
});

describe('四位一節', () => {
  it.each([
    [10000, '壹萬元整'],
    [12345, '壹萬貳仟參佰肆拾伍元整'],
    [100000000, '壹億元整'],
    [738150, '柒拾參萬捌仟壹佰伍拾元整'],
  ])('%i → %s', (amount, expected) => {
    expect(amountToChinese(amount)).toBe(expected);
  });

  it('設計稿的金額', () => {
    // pdf/code.html 顯示 NT$ 738,150
    expect(formatAmountInWords(738150)).toBe('新台幣 柒拾參萬捌仟壹佰伍拾元整');
  });
});

describe('零的處理', () => {
  it('中間的零要寫出來', () => {
    // 少了零會讀成一百零一萬 vs 一百一十萬
    expect(amountToChinese(1000001)).toBe('壹佰萬零壹元整');
    expect(amountToChinese(10001)).toBe('壹萬零壹元整');
  });

  it('連續的零只寫一個', () => {
    expect(amountToChinese(100001)).toBe('壹拾萬零壹元整');
  });

  it('尾端的零不寫', () => {
    expect(amountToChinese(1100)).toBe('壹仟壹佰元整');
    expect(amountToChinese(1200000)).toBe('壹佰貳拾萬元整');
  });

  it('整節為零時補零', () => {
    // 壹億零捌，不是壹億捌
    expect(amountToChinese(100000008)).toBe('壹億零捌元整');
  });

  it('跨節的零不重複', () => {
    const result = amountToChinese(100000000);
    expect(result).toBe('壹億元整');
    expect(result).not.toContain('零零');
  });
});

describe('實務金額', () => {
  it.each([
    [686614, '陸拾捌萬陸仟陸佰壹拾肆元整'],
    [34331, '參萬肆仟參佰參拾壹元整'],
    [703000, '柒拾萬參仟元整'],
    [35150, '參萬伍仟壹佰伍拾元整'],
  ])('%i → %s', (amount, expected) => {
    expect(amountToChinese(amount)).toBe(expected);
  });
});

describe('輸入驗證', () => {
  it('小數拋錯而非自行進位', () => {
    // 替呼叫端四捨五入會讓大寫與阿拉伯數字對不起來，
    // 而那正是這個欄位要防範的事
    expect(() => amountToChinese(100.5)).toThrow('必須為整數');
  });

  it('負數加負字', () => {
    expect(amountToChinese(-100)).toBe('負壹佰元整');
  });

  it('非數值回空字串', () => {
    expect(amountToChinese(NaN)).toBe('');
    expect(amountToChinese(Infinity)).toBe('');
  });

  it('超過範圍拋錯', () => {
    expect(() => amountToChinese(1e17)).toThrow('超過可表示範圍');
  });
});

describe('不產生無效字串', () => {
  it('大量隨機金額都不含連續零或以零結尾', () => {
    for (let i = 0; i < 2000; i++) {
      const amount = Math.floor(Math.random() * 100_000_000);
      const text = amountToChinese(amount);

      expect(text, `${amount} 產生連續零`).not.toContain('零零');
      expect(text, `${amount} 以零元整結尾`).not.toMatch(/零元整$/);
      expect(text.endsWith('元整'), `${amount} 缺少元整`).toBe(true);
    }
  });

  it('金額越大字串不會變短', () => {
    // 粗略的單調性檢查，可抓出進位邏輯的斷裂
    let previous = 0;
    for (const amount of [1, 10, 100, 1000, 10000, 100000, 1000000]) {
      const length = amountToChinese(amount).length;
      expect(length, `${amount} 的字串比前一級短`).toBeGreaterThanOrEqual(previous);
      previous = length;
    }
  });
});
