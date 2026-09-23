// inventory: fmom_mm_det
// varexo_det lead in a moment row
var y c;
varexo e;
varexo_det tau;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_moments;
tau(1);
end;
