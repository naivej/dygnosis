// compare: symbol metadata added, changed, and removed
var c $C$ (long_name='consumption')
    k
    n (long_name='hours')
    z (long_name='z')
    c_hat
    w (long_name='wage');
varexo e;
parameters beta;
beta = 0.9;
heterogeneity_dimension d;
var(heterogeneity=d) h (long_name='labor');
model;
c = beta*c(+1)+k+n+z+c_hat+e;
end;
shocks;
var e; periods 1; values 0.2;
end;
