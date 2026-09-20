// inventory: e359_svar_identification_equation_twice
// 7.1 refuses this file: `equation number 1 referenced more than once under a single lag.`
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
equation 1, Pie, Y;
equation 1, Pie;
end;
