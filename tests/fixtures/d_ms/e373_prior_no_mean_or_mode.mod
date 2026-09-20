// inventory: e373_prior_no_mean_or_mode
// 7.1 refuses this file: `You must pass at least one of mean and mode to the prior statement.`
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
alpha.prior(shape=beta, stdev=0.1);
