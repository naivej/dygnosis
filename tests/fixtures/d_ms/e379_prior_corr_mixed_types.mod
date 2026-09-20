// inventory: e379_prior_corr_mixed_types
// 7.1 refuses this file: `In the corr(A,B).prior statement, A and B must be of the same type. In your case, Pie and eps are of different types.`
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
corr(Pie, eps).prior(shape=beta, mean=0.5, stdev=0.1);
