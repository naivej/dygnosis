// inventory: e356_identification_lag_no_equation
// A second `exclusion lag 0;` where the first one still needs its `equation`
// row: 7.1 refuses `syntax error, unexpected EXCLUSION, expecting EQUATION`.
// The sibling `d_ms/e001_identification_lag_no_equation.mod` closes the body with
// `end;` instead, which 7.1 refuses with `unexpected END, expecting EQUATION`;
// both spellings are swept and locked.
var R Pie Y;
varexo eps;
parameters alpha beta;
alpha = 0.36;
beta = 0.99;
model;
R = beta*R(-1) + eps;
Pie = alpha*R(-1) + eps;
Y = beta*Pie(-1) + eps;
end;
initval;
R = 0;
Pie = 0;
Y = 0;
end;
shocks;
var eps; stderr 0.1;
end;
varobs Y Pie R;
svar_identification;
exclusion lag 0;
exclusion lag 0;
end;
