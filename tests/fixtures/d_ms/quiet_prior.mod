// inventory: d_ms_quiet_prior
// The dotted `prior` statement in every head 7.1 keys on a declared symbol,
// plus the `options` / `subsamples` heads, whose bodies we claim but never read.
var y c k;
varexo e;
parameters alpha beta;
alpha = 0.36;
beta = 0.99;
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
alpha.prior(shape=beta, mean=0.5, stdev=0.1);
beta.prior(shape=gamma, mode=0.4, variance=0.01, domain=[0, 1]);
std(e).prior(shape=inv_gamma, mean=0.1, stdev=0.2);
corr(y,c).prior(shape=beta, mean=0.3, variance=0.01);
[alpha, beta].prior(shape=inv_gamma, mean=[0.5, 0.6], stdev=1.0, domain=[0.1 0.2 0.3 0.4]);
alpha.prior = beta.prior;
alpha.options(init=1, jscale=0.5);
alpha.options(bounds=[0, 1]);
alpha.subsamples(y = 1959Q1:2005Q4);
