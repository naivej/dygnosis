// inventory: lists_e239_toplevel_forecast
// A top-level `xx = 3;` is no declaration, so the list names 7.1-unknown `xx`.
xx = 3;
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

forecast xx;
