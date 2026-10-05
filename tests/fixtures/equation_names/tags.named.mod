var y;
model;
    # x = 1;
    [static]
    y = 1;
    [name='eq1']
    y = y(-1);
    [dynamic, group='g', name='eq2']
    y = y(-1);
end;
