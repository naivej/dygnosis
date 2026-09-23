// inventory: fmom_wper_minus
// negative weight period
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;
matched_irfs_weights;
y(-1), e, c(1), e, 1;
end;
