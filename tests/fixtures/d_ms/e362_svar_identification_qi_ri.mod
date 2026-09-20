// inventory: e362_svar_identification_qi_ri
// 7.1 refuses this file: `SVAR_IDENTIFICATION: a single restrictions must affect either Qi or Ri, but not both`
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
restriction equation 1, coeff(Pie,0) + coeff(Y,1) = 0;
end;
