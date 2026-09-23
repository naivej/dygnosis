// inventory: fmom_wt_helper
// external function as a weights-row weight
var y c;
varexo e;
varexo_det tau;
parameters a;
trend_var(growth_factor=1.01) A;
external_function(name=helper);
a = 0.5;

model;
y = a*y(-1) + e + A;
c = y;
#loc = y;
end;
matched_irfs_weights;
y(1), e, c(1), e, helper;
end;
