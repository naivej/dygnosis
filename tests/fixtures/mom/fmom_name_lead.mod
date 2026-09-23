// inventory: fmom_name_lead
// lead where a name belongs
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs;
var y(1); varexo e; periods 1; values 1;
end;
