// inventory: e001_joint_prior_scalar_mean
// `[alpha, beta].prior(mean=0.5, …);` — the joint form takes a vector; 7.1 refuses `unexpected FLOAT_NUMBER, expecting '['`.
var y c k;
varexo e;
parameters alpha beta gamma;
alpha = 0.36;
beta = 0.99;
gamma = 0.5;
model;
c = alpha*y + beta*c(-1) + e;
y = beta*y(-1) + c;
k = y;
end;
initval;
y = 0;
c = 0;
k = 0;
end;
shocks;
var e; stderr 0.1;
end;
[alpha, beta].prior(shape=beta, mean=0.5, stdev=0.1);
