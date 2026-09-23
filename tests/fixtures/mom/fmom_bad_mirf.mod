// inventory: fmom_bad_mirf
// stray row beside a matched_irfs row
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs;
var y; varexo e; periods 1; values 1;
y;
end;
