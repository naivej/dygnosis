// inventory: e058_prior_std_undeclared
// 7.1 refuses this file: `Unknown symbol: nosuchvar.`
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
std(nosuchvar).prior(shape=beta, mean=0.5, stdev=0.1);
