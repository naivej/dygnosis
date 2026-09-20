// inventory: e374_prior_stdev_and_variance
// 7.1 refuses this file: `You must pass exactly one of stdev and variance to the prior statement.`
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
alpha.prior(shape=beta, mean=0.5, stdev=0.1, variance=0.01);
