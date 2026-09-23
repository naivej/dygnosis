// inventory: fmom_ex_lead
// lead in a value
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs;
var y; varexo e; periods 1; values (y(1));
end;
