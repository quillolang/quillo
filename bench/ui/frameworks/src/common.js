// What every implementation shares: the same labels as Quillo's and vanilla's.
const ADJ = ["pretty", "large", "big", "small", "tall", "short", "long", "handsome", "plain", "quaint", "clean", "elegant", "easy", "angry", "crazy", "helpful", "mushy", "odd", "unsightly", "adorable", "important", "inexpensive", "cheap", "expensive", "fancy"];
const COLOR = ["red", "yellow", "blue", "green", "pink", "brown", "purple", "brown", "white", "black", "orange"];
const NOUN = ["table", "chair", "house", "bbq", "desk", "car", "pony", "cookie", "sandwich", "burger", "pizza", "mouse", "keyboard"];
export const label = (i) => `${ADJ[(i * 7) % ADJ.length]} ${COLOR[(i * 13) % COLOR.length]} ${NOUN[(i * 17) % NOUN.length]}`;
let next = 1;
export function buildData(n, make = (id, l) => ({ id, label: l })) {
  const d = new Array(n);
  for (let i = 0; i < n; i++) { d[i] = make(next, label(next)); next++; }
  return d;
}
