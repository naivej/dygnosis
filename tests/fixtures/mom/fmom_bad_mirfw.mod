// inventory: fmom_bad_mirfw
// stray row beside a weights row
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs_weights;
y(1), e, c(1), e, 1;
y;
end;
