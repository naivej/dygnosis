// inventory: mom_e386_unsupported_operator
// A `matched_moments` row that is a sum. 7.1's walk refuses: `Unsupported binary
// operator` — a moment is a product, power, or single variable.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

matched_moments;
y + c;
end;
