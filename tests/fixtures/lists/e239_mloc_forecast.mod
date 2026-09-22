// inventory: lists_e239_mloc_forecast
// A top-level `# mloc = 3;` is no declaration either, so the name stays unknown.
# mloc = 3;
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

forecast mloc;
