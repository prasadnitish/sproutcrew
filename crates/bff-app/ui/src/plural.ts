/** A count with its noun: "1 project", "3 projects". */
export const count = (n: number, one: string, many = `${one}s`): string => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

/** The decision inbox's count in words: "1 item needs input", "3 items need input". */
export const needInput = (n: number): string => (n === 1 ? "1 item needs input" : `${n.toLocaleString("en-US")} items need input`);
