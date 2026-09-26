// compare: different nonempty names with the same text do not pair
var y;
heterogeneity_dimension h;
var(heterogeneity=h) a;
model;
[name='anchor'] y = 0;
end;
model(heterogeneity=h);
[name='alpha'] a = 1;
end;
