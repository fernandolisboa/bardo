---
id: jobs
title: Tarefas
group: reference
place: jobs
tour: jobs
---

# Tarefas

Trabalho longo roda como **tarefa**: pesquisa, roteiro, narração, imagens, clipes, render, exportação, envio, sincronização de métricas. As tarefas rodam em segundo plano, então você continua trabalhando enquanto elas andam, e elas sobrevivem ao Bardo fechar. [Mostre o painel](bardo:tour/jobs).

<a id="panel"></a>
## O painel Tarefas

**Tarefas** na navegação abre um painel ao lado da tela em que você está (por cima do lado direito dela, numa janela estreita), e fecha de novo; a linha ao lado diz quantas tarefas estão rodando ou esperando. O painel agrupa as tarefas em **Em execução**, **Na fila**, **Com falha** e **Encerradas**, cada cartão com o que a tarefa faz e onde ela está. Uma tarefa que espera a hora dela, como um post agendado, diz quando sai.

<a id="progress"></a>
## Progresso

Uma tarefa em execução mostra uma barra e o percentual. As telas também mostram as próprias tarefas: uma etapa diz que está gerando, e o resultado aparece quando a tarefa termina, sem precisar olhar o painel.

<a id="cancel"></a>
## Cancelar

**Cancelar** para uma tarefa em execução ou na fila. O que ela já fez fica guardado para uma nova tentativa, e uma chamada que o provedor já respondeu está paga e conta em [Custos](costs.md#spent). Um render ou uma exportação cancelada guarda as redes que já terminou, e uma nova tentativa faz as que faltam.

Um texto que o Claude ainda está escrevendo (um roteiro, um plano de cenas, um título) é abandonado quando a tarefa é cancelada: o Bardo para de ler e fecha a conexão em vez de esperar o fim. O Claude pode cobrar pelo que escreveu até ali, então [Custos](costs.md#spent) conta uma estimativa do texto inteiro, que pode ser mais do que custou um texto cancelado cedo. Um plano de cenas cuja resposta chega bem na hora em que você cancela não é aplicado ao projeto; ele está pago, e **Tentar de novo** nessa tarefa, ou **Planejar cenas** na etapa Cenas, faz o plano a partir dessa resposta sem pedir de novo ao Claude.

<a id="retry"></a>
## Tentar de novo

Quando um provedor falha por um motivo passageiro (uma conexão que não pôde ser feita, servidor ocupado, limite de requisições), o Bardo tenta de novo sozinho, até quatro tentativas no total, esperando mais a cada vez, e o cartão diz qual tentativa falhou. Quando a falha é duradoura, como uma chave que falta ou um login recusado, a tarefa para como **Com falha**, com o que deu errado e **Detalhes**.

Um pedido pago de texto, imagem, voz ou música que foi enviado e ficou sem resposta não é repetido sozinho: o provedor pode ter feito o trabalho e cobrado por ele. A tarefa para como **Com falha** e diz isso, e [Custos](costs.md#spent) conta uma estimativa do que ele pode ter custado. Veja [Um pedido pago ficou sem resposta](troubleshooting.md#answer-lost).

Um pedido de clipe que o provedor aceitou e que depois terminou sem clipe (falhou, foi cancelado ou esquecido do lado dele) também não é repetido sozinho, porque enviar de novo é um pedido novo. A cena mostra a falha, a tarefa para como **Com falha** quando as outras cenas terminam, e é o **Tentar de novo** que envia a cena outra vez ([Clipes](clips.md)).

**Tentar de novo**, numa tarefa com falha ou cancelada, coloca ela na fila de novo. Ela continua do último ponto que salvou, não do começo, e uma tarefa de imagens ou de clipes faz só as cenas que ainda não entregou: o que você fez depois com uma imagem ou um clipe já entregue (rejeitou, substituiu) não o traz de volta. Uma tarefa de imagens ou de clipes não é tentada de novo enquanto outra trabalha nas mesmas cenas: espere essa terminar, para nenhuma cena ser paga duas vezes. Resolva antes o que a falha aponta: veja [Solução de problemas](troubleshooting.md).

Uma tarefa de plano de cenas ou de imagens não volta para a fila enquanto outra tarefa de cenas do mesmo projeto está na fila, em execução ou ainda parando: as duas pagariam pelo mesmo trabalho. E uma tarefa de plano de cenas mais antiga não volta para a fila depois que uma tarefa de cenas mais nova do projeto veio depois dela, porque poderia substituir o plano feito depois: tente de novo a tarefa mais nova, ou planeje de novo pela etapa [Cenas](scenes.md), que pergunta antes.

<a id="restart"></a>
## Fechar o Bardo no meio de uma tarefa

Fechar o Bardo, ou desligar o computador, nunca perde uma tarefa. Na próxima vez que o Bardo abrir, cada tarefa em execução continua do último ponto que salvou, e a tentativa em que estava não conta nas novas tentativas. Uma tarefa que já tinha entregado trabalho a um provedor (um clipe sendo feito, um envio sendo processado) pergunta ao provedor como ficou, em vez de pagar duas vezes. Uma tarefa que esperava a resposta de um pedido pago de texto, imagem, voz ou música, sobre o qual não dá para perguntar, não o envia de novo: ela para como **Com falha**, como num [pedido pago que ficou sem resposta](troubleshooting.md#answer-lost), e [Custos](costs.md#spent) conta uma estimativa do que ele pode ter custado. Uma tarefa esperando nova tentativa, ou esperando a hora dela, continua esperando.

<a id="test"></a>
## Tarefas de teste

**Iniciar tarefa de teste** roda uma contagem de dez segundos que não faz nada e não custa nada, para testar progresso, cancelamento, nova tentativa e o Bardo fechando no meio. **Iniciar tarefa de teste com falha** falha de propósito, para você ver uma nova tentativa automática e uma falha.
