const DIGITS = ["零", "壹", "贰", "叁", "肆", "伍", "陆", "柒", "捌", "玖"] as const;
const SMALL_UNITS = ["", "拾", "佰", "仟"] as const;
const GROUP_UNITS = ["", "万", "亿", "兆", "京"] as const;

const DIGIT_VALUES = new Map<string, number>(DIGITS.map((digit, value) => [digit, value]));
const SMALL_UNIT_VALUES = new Map<string, bigint>([
  ["拾", 10n],
  ["佰", 100n],
  ["仟", 1000n],
]);
const GROUP_UNIT_VALUES = new Map<string, bigint>([
  ["万", 10_000n],
  ["亿", 100_000_000n],
  ["兆", 1_000_000_000_000n],
  ["京", 10_000_000_000_000_000n],
]);
const MONEY_DIGIT_PATTERN = "零壹贰叁肆伍陆柒捌玖";

function convertGroup(group: string): string {
  let result = "";
  let zeroPending = false;
  const padded = group.padStart(4, "0");

  for (let index = 0; index < padded.length; index += 1) {
    const digit = Number(padded[index]);
    const unitIndex = padded.length - index - 1;
    if (digit === 0) {
      if (result) zeroPending = true;
      continue;
    }
    if (zeroPending) result += DIGITS[0];
    result += `${DIGITS[digit]}${SMALL_UNITS[unitIndex]}`;
    zeroPending = false;
  }
  return result;
}

function integerToChinese(integer: string): string {
  if (integer.length > GROUP_UNITS.length * 4) {
    throw new Error(`整数部分最多支持 ${GROUP_UNITS.length * 4} 位`);
  }

  const groups: string[] = [];
  for (let end = integer.length; end > 0; end -= 4) {
    groups.push(integer.slice(Math.max(0, end - 4), end));
  }

  let result = "";
  let zeroPending = false;
  for (let index = groups.length - 1; index >= 0; index -= 1) {
    const value = Number(groups[index]);
    if (value === 0) {
      if (result) zeroPending = true;
      continue;
    }
    if (result && (zeroPending || value < 1000)) result += DIGITS[0];
    result += `${convertGroup(groups[index])}${GROUP_UNITS[index]}`;
    zeroPending = false;
  }
  return result || DIGITS[0];
}

export function arabicToChineseMoney(input: string): string {
  const raw = input.trim();
  if (/^[+-]?\d+\.\d{3,}$/.test(raw)) throw new Error("人民币金额最多支持两位小数");

  const match = /^([+-]?)(\d+)(?:\.(\d{1,2}))?$/.exec(raw);
  if (!match) throw new Error("请输入人民币金额，可包含负号和最多两位小数");

  const [, sign, integerRaw, fractionRaw = ""] = match;
  const integer = integerRaw.replace(/^0+(?=\d)/, "");
  const fraction = fractionRaw.padEnd(2, "0");
  const jiao = Number(fraction[0] ?? 0);
  const fen = Number(fraction[1] ?? 0);
  const isZero = integer === "0" && jiao === 0 && fen === 0;

  let fractionText = "";
  if (jiao === 0 && fen === 0) {
    fractionText = "整";
  } else {
    if (jiao > 0) fractionText += `${DIGITS[jiao]}角`;
    if (jiao === 0 && fen > 0) fractionText += DIGITS[0];
    if (fen > 0) fractionText += `${DIGITS[fen]}分`;
  }

  return `${sign === "-" && !isZero ? "负" : ""}${integerToChinese(integer)}元${fractionText}`;
}

function parseChineseInteger(input: string): bigint {
  if (!input) throw new Error("缺少元之前的中文大写金额");
  if (input === DIGITS[0]) return 0n;

  let total = 0n;
  let section = 0n;
  let pendingDigit: number | null = null;
  let lastSmallUnit = 10_000n;
  let lastGroupUnit = 100_000_000_000_000_000_000n;
  let meaningful = false;

  for (const char of input) {
    const digit = DIGIT_VALUES.get(char);
    if (digit !== undefined) {
      if (pendingDigit !== null && pendingDigit !== 0) throw new Error("中文大写金额格式不正确");
      pendingDigit = digit;
      if (digit !== 0) meaningful = true;
      continue;
    }

    const smallUnit = SMALL_UNIT_VALUES.get(char);
    if (smallUnit !== undefined) {
      if (smallUnit >= lastSmallUnit) throw new Error("中文大写金额单位顺序不正确");
      const value = pendingDigit ?? (smallUnit === 10n && section === 0n ? 1 : 0);
      if (value === 0) throw new Error("中文大写金额格式不正确");
      section += BigInt(value) * smallUnit;
      pendingDigit = null;
      lastSmallUnit = smallUnit;
      meaningful = true;
      continue;
    }

    const groupUnit = GROUP_UNIT_VALUES.get(char);
    if (groupUnit !== undefined) {
      if (groupUnit >= lastGroupUnit) throw new Error("中文大写金额单位顺序不正确");
      section += BigInt(pendingDigit ?? 0);
      if (section === 0n) throw new Error("中文大写金额格式不正确");
      total += section * groupUnit;
      section = 0n;
      pendingDigit = null;
      lastSmallUnit = 10_000n;
      lastGroupUnit = groupUnit;
      meaningful = true;
      continue;
    }

    throw new Error(`不支持的金额字符：${char}`);
  }

  if (!meaningful || pendingDigit === 0) {
    if (pendingDigit === 0 && (total !== 0n || section !== 0n)) return total + section;
    throw new Error("中文大写金额格式不完整");
  }
  return total + section + BigInt(pendingDigit ?? 0);
}

function parseFraction(input: string): [number, number] {
  if (!input || input === "整" || input === "正") return [0, 0];

  const angleMatch = new RegExp(
    `^([${MONEY_DIGIT_PATTERN}])角(?:零?([${MONEY_DIGIT_PATTERN}])分)?$`,
  ).exec(input);
  if (angleMatch) {
    return [DIGIT_VALUES.get(angleMatch[1])!, DIGIT_VALUES.get(angleMatch[2] ?? DIGITS[0])!];
  }

  const fenMatch = new RegExp(`^零?([${MONEY_DIGIT_PATTERN}])分$`).exec(input);
  if (fenMatch) return [0, DIGIT_VALUES.get(fenMatch[1])!];

  throw new Error("元之后请使用“整”或“角/分”金额格式");
}

export function chineseMoneyToArabic(input: string): string {
  let raw = input.trim();
  if (!raw) throw new Error("请输入包含“元”的中文大写金额");
  if (raw.startsWith("人民币")) raw = raw.slice(3);

  const negative = raw.startsWith("负");
  if (negative) raw = raw.slice(1);

  const match = /^(.+?)[元圆](.*)$/.exec(raw);
  if (!match) throw new Error("中文大写金额必须包含“元”");

  const integer = parseChineseInteger(match[1]);
  const [jiao, fen] = parseFraction(match[2]);
  const isZero = integer === 0n && jiao === 0 && fen === 0;
  return `${negative && !isZero ? "-" : ""}${integer}.${jiao}${fen}`;
}
