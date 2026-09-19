// inventory: d_ms_kept_extras
// The extras that already reached this family through the skip path stay:
// E271 (option declared twice), W201 (`restriction_fname` deprecated), and
// W160 (a named `datafile` that is not on disk — `a.csv` here).
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
sbvar(freq=4, freq=5);
markov_switching(chain=1, chain=2, number_of_regimes=2, duration=2.5);
data(file='a.csv', file='b.csv');
ms_estimation(nlags=2, nlags=3);
conditional_forecast(periods=4, periods=5, parameter_set=calibration);
svar(coefficients, coefficients, chain=1);
alpha.prior(shape=beta, mean=0.5, mean=0.6, stdev=0.1);
sbvar(restriction_fname=foo, freq=4);
