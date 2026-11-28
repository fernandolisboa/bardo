---
id: api-keys
title: Suas chaves de API
group: getting-started
place: settings/keys
tour: keys
---

# Suas chaves de API

O Bardo não tem IA própria nem conta para criar. Ele trabalha com as suas contas em cada provedor, por meio de chaves de API que você cria lá e salva uma única vez em [Configurações › Chaves de API](bardo:go/settings/keys). Você paga cada provedor diretamente, pelos preços dele. [Mostre a aba](bardo:tour/keys).

<a id="providers"></a>
## O que cada provedor faz

- **Claude** escreve roteiros, títulos, descrições, ideias de vídeo, planos de cena e os prompts para os modelos de imagem e vídeo. Você precisa dele para o roteiro, as cenas, as ideias e os textos dos posts.
- **ElevenLabs** narra, lista as suas vozes (clones incluídos) e compõe a música do vídeo. Você precisa dela para a narração e as vozes das personas, e para a música só se gerar no Bardo (plano pago da ElevenLabs).
- **Gemini** gera as imagens das cenas (Nano Banana) e clipes de vídeo (Gemini Omni Flash). Você precisa dele para as imagens das cenas e para desenhar a [aparência](personas.md#look) de uma persona e, para os clipes, só se usar os modelos do Google.
- **Higgsfield** gera clipes de vídeo pelos modelos que oferece. Você só precisa dela para clipes, se escolher os modelos dela.
- **Plano Higgsfield** gera clipes de vídeo pagos com os créditos do seu plano no higgsfield.ai, sem chave de API. Você faz login nele pelo navegador ([abaixo](#higgsfield-plan)).
- **Kling** gera clipes de vídeo direto dos modelos do próprio Kling (Kling 3.0, 2.6 e 2.5 Turbo), e clipes interpretados com o Kling 3.0 Motion Control ([Captura de atuação](capture.md#kling)). Você só precisa dela se escolher os modelos dele.
- **BytePlus ModelArk** gera clipes interpretados com o Seedance 2.5 direto do ModelArk, cuja versão final completa o rascunho aprovado. Você só precisa dela para clipes interpretados, se escolher esse modelo de tomada. O cartão dela recebe a chave do ModelArk e um bucket seu de armazenamento de objetos para os arquivos da tomada.
- **TypeSafe (JEV)** é o motor de decisão: ranqueia ideias de vídeo e pontua sugestões de corte. Você precisa dele para ranquear ideias e sugerir cortes.
- **YouTube Data API** traz a pesquisa de nicho e as estatísticas públicas dos vídeos. Você precisa dela para a pesquisa e para os números públicos dos seus posts.

Você não precisa de todas as chaves no primeiro dia. Uma tela que depende de uma chave que falta avisa. Clipes são opcionais: uma cena sem clipe fica com a imagem parada.

<a id="higgsfield-plan"></a>
## O plano Higgsfield

A Higgsfield vende duas coisas separadas: a API, com saldo em dólar e chave (o cartão **Higgsfield**), e os planos do higgsfield.ai, com créditos mensais. O Bardo pode gastar os créditos do plano em clipes pelo servidor MCP oficial da Higgsfield, que os termos da Higgsfield abrem para apps como o Bardo. O cartão dele fica em **Planos com login**, abaixo das chaves.

1. No cartão **Plano Higgsfield**, clique em **Fazer login**. O Bardo abre a página da Higgsfield no navegador.
2. Entre com a sua conta do higgsfield.ai e permita o Bardo. A página diz que o Bardo está conectado; feche a aba.
3. O cartão diz **Com login** e mostra o seu plano e os créditos restantes. Depois que o Bardo reinicia, **Ver créditos** lê de novo.

Depois, escolha um modelo marcado **Plano Higgsfield** para os seus clipes ([Modelo de vídeo](clips.md#model)). Antes de cada clipe, o Bardo pergunta à Higgsfield quanto ele custa em créditos e recusa quando o plano não consegue pagar. Cada clipe é enviado uma vez só: se a resposta se perde no caminho, o Bardo procura o clipe na sua biblioteca da Higgsfield em vez de enviar de novo, e quando a biblioteca não o mostra a cena pede que você confira a biblioteca antes de animá-la de novo. Cada clipe fica registrado em [Custos](bardo:go/costs) pelas tarifas por segundo dos modelos do plano, calculadas a US$ 0,049 por crédito (quanto custa um crédito no plano Plus). Em outro plano, edite essas tarifas em Custos.

O login fica no Gerenciador de Credenciais do Windows, como uma chave, e é renovado sozinho. Se a Higgsfield deixar de aceitá-lo, o cartão diz **Login expirado**; clique em **Fazer login de novo** e tente de novo, em [Tarefas](jobs.md), os clipes que pararam por isso. **Encerrar login** esquece o login no Bardo; para encerrar o acesso do Bardo na Higgsfield também, remova-o na sua conta da Higgsfield.

<a id="decision-engine"></a>
## Motor de decisão

O **Motor de decisão**, no alto da aba, escolhe o motor que ranqueia as ideias de vídeo em [Temas](themes-ranking.md) e pontua as [sugestões de corte](editor.md) no editor. O motor chama com a chave do provedor dele, desta aba; a estimativa conta no orçamento desse provedor, e os custos ficam registrados nele.

Por enquanto há um motor, o **JEV (TypeSafe)**, que usa a sua chave da TypeSafe. Uma nova escolha vale para o próximo ranqueamento ou pontuação; uma tarefa já na fila termina com o motor com que começou. A escolha vai num [arquivo de configurações](backup.md#settings-file) junto com as suas outras preferências.

<a id="step-by-step"></a>
## Passo a passo de cada chave

O **Passo a passo** de cada cartão abre uma tela que guia você até a chave daquele provedor: onde criar a conta, qual página gera a chave (com um link direto para ela), o que marcar e como a chave se parece. O último passo é o próprio campo do cartão, então você cola, salva e testa a chave sem sair dessa tela. **Voltar** leva de volta à aba.

Os mesmos passos estão em [Como obter cada chave de API](get-api-keys.md).

<a id="where-kept"></a>
## Onde as chaves ficam

As chaves ficam no Gerenciador de Credenciais do Windows, na sua conta do Windows, nunca no banco de dados do Bardo. Veja [Onde ficam seus dados e chaves](data-and-keys.md#credentials).

Remover uma chave nas Configurações apaga ela do Gerenciador de Credenciais. Para desativar a chave de vez, revogue também no painel do próprio provedor.

Um projeto pode usar uma chave própria para um provedor no lugar da salva aqui, com orçamento próprio: veja [As chaves próprias de um projeto](project-keys.md).

<a id="masked"></a>
## O que o Bardo mostra de uma chave

Depois de salva, uma chave nunca mais aparece: o cartão dela diz **Chave salva, terminada em** e os quatro últimos caracteres, para você saber qual chave é. Sempre que um texto sai do app (o log, mensagens de erro, a tela), uma chave salva aparece mascarada, até dentro da mensagem do próprio provedor. Para trocar uma chave, cole a nova e use **Trocar chave**.

<a id="testing"></a>
## Testar uma chave

**Testar chave** faz a chamada mais barata que o provedor oferece e que exige uma chave válida, e diz o que encontrou: a chave funciona, o provedor recusou, falta uma permissão ou uma API ativada, ou uma cota ou falta de créditos está bloqueando agora. Um teste da YouTube Data API usa 1 unidade da sua cota diária.

<a id="budgets"></a>
## Custos e orçamentos

Cada geração registra quanto custou, pelo valor que o provedor informou ou estimado pela tabela de preços dele. Em [Custos](bardo:go/costs) você vê quanto gastou no mês, por provedor, por canal e por vídeo.

Defina um **orçamento** mensal para cada provedor na tela Custos. A partir de 80% a estimativa de uma geração avisa, e quando um orçamento é atingido, cada tarefa nova desse provedor pergunta antes de começar. Nada é interrompido no meio. Veja [Custos e orçamentos](costs.md#budgets).
