// inventory: e357_svar_identification_two_cholesky
// 7.1 refuses this file: `Within the svar_identification statement, you may only have one of upper_cholesky and lower_cholesky.`
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
upper_cholesky;
lower_cholesky;
end;
