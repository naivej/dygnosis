// inventory: mom_matched_moments_rows
// Original AR(1) plus a definition. `matched_moments` rows are model expressions
// separated by `;`: a bare name, a product, and a product with a lag.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

matched_moments;
y;
c*y;
y*y(-1);
end;
