// inventory: fmom_mirf_zoom
// bad option word
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs(zoom);
var y; varexo e; periods 1; values 1;
end;
